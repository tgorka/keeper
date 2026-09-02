//! IPC command layer for the keeper shell (AD-8, AD-21).
//!
//! This is the place where [`CoreError`] is mapped to the `IpcError` envelope,
//! where the bulk of the `#[tauri::command]`s live, and where the concrete
//! [`Platform`] port is implemented. The app-lifecycle command is the one
//! deliberate peer seam — it lives in [`crate::lifecycle`] (Epic 14-1) so the
//! single Rust lifecycle entry point is self-contained. No business logic lives
//! in either module — commands delegate to `keeper-core`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, FixedOffset, Local, TimeZone, Timelike};
use keeper_core::account::AccountManager;
use keeper_core::archive::recordings::{
    fallback_session_id, relative_session_path, RecordingRow, RecordingSegmentRow,
};
use keeper_core::auth;
use keeper_core::auth::BeeperFlowRegistry;
use keeper_core::demo::snapshot_then_diff;
use keeper_core::egress::{compute_egress, EGRESS_UPDATE_ENDPOINT};
use keeper_core::error::{
    AccountError, ArchiveError, AuthError, BackupError, BridgeError, CoreError, InboxError,
    MediaError, PlatformError, RecordingError, SendError, SignalError, TimelineError,
    VerificationError,
};
#[cfg(desktop)]
use keeper_core::notes::frontmatter::Frontmatter;
#[cfg(desktop)]
use keeper_core::notes::recording_note::{self, NoteStub, SessionFacts};
use keeper_core::notes::NotesError;
use keeper_core::oauth::OAuthFlowRegistry;
use keeper_core::platform::Platform;
#[cfg(desktop)]
use keeper_core::platform::SecretCache;
use keeper_core::recording::path_template::{
    PathTemplate, RelativePath, RenderCtx, DEFAULT_TEMPLATE,
};
use keeper_core::recording::{
    current_segment_bytes_on_disk, evaluate_destination, plan_disk_guard_action,
    recover_orphaned_sessions, resolve_recording_permission, resolve_screen_recording_access,
    resolve_source_access, session_bytes_on_disk, ApplicationTarget, CameraSelection,
    CaptureTarget, DiskGuardAction, DiskGuardLatch, ManifestStatus, MicSelection, Recorder,
    RecordingEvent, RecordingSession, SegmentEntry, SessionDevices, SessionManifest, SessionParams,
    SessionState, RECORDING_MIN_FREE_BYTES, RECORDING_WARN_FREE_BYTES, RECOVERY_MAX_DEPTH,
    RECOVERY_MAX_VISITS,
};
use keeper_core::vm::{
    AccountVm, ApprovalDraftVm, BackupStatus, BbctlAvailabilityVm, BbctlProgressVm,
    BridgeDiscoveryVm, BridgeHealthSnapshot, BridgeLoginInput, BridgeLoginVm, BridgeNetworkVm,
    CapabilitiesVm, ChatNotifyMode, ConfigLayersVm, ConnectionStatusBatch, CouplingCaveatVm,
    DemoBatch, DockBadgeMode, DraftMirrorBatch, EditVersionVm, EgressEndpointVm,
    EncryptionStatusBatch, ExportPhase, ExportProgressVm, ExportRequestVm, HotkeyVm, InboxBatch,
    IncognitoVm, IpcError, IpcErrorCode, MenuSectionVm, NavState, NetworksSnapshot,
    NewChatResolutionVm, NotificationPermission, NotifyTarget, OutboxVm, PaginationStatusBatch,
    PaletteMode, PaletteResultsVm, PingVm, Provider, RecordingDestinationKind,
    RecordingDurabilityState, RecordingDurabilityVm, RecordingFilterVm, RecordingNoteStubVm,
    RecordingNoteTargetVm, RecordingPathPreviewVm, RecordingPermissionVm, RecordingProfileVm,
    RecordingSearchVm, RecordingSessionMetaVm, RecordingSettingsVm, RecordingSourcesVm,
    RecordingStatusVm, RecordingSummaryVm, RecordingTargetVm, RecordingUiState,
    RecordingVolumeState, RecordingVolumeVm, RemoteDraftVm, ResolveSupportVm, RoomListBatch,
    ScreenRecordingAccess, SearchFilterVm, SearchHitVm, SpacesSnapshot, SyncListSettingsVm,
    TccPermission, TimelineBatch, TypingBatch, VerificationFlowVm,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;
use ts_rs::TS;

/// The build-target [`Recorder`] impl behind the recording IPC commands (Story
/// 16.5). The `Recorder` trait is not object-safe (its async methods are RPITIT),
/// so the concrete type is selected at compile time per target rather than held
/// as a trait object: the desktop recorder spawns `keeper-rec` per round-trip;
/// the iOS one honestly answers [`CoreError::Unsupported`] (the frontend never
/// calls these commands there — the `recording` capability is `false`).
#[cfg(desktop)]
type PlatformRecorder = crate::recorder::DesktopRecorder;
#[cfg(target_os = "ios")]
type PlatformRecorder = crate::recorder::IosRecorder;

/// Tauri-managed application state holding the injected platform port and the
/// single-account supervisor.
///
/// Keeps the concrete [`Platform`] behind a trait object so the command layer
/// depends only on the port, never a concrete type (AD-24). The
/// [`AccountManager`] owns the live `Client`/`SyncService` and per-subscription
/// tasks (AD-19).
pub struct AppState {
    pub platform: Arc<dyn Platform>,
    pub accounts: AccountManager,
    /// In-flight OIDC (OAuth 2.0 / MSC3861) callback registry (Story 2.2). The
    /// deep-link `on_open_url` handler resolves incoming `keeper://oauth/callback`
    /// URLs against it; each `login_oidc` call registers its pending flow here,
    /// and `cancel_oidc` aborts all pending flows.
    pub oauth_flows: Arc<OAuthFlowRegistry>,
    /// In-flight Beeper email-code login registry (Story 2.3). Holds the
    /// intermediate login-request id between `beeper_request_code` and
    /// `login_beeper` (keyed by email) so it never crosses IPC; `cancel_beeper`
    /// drops one email's entry. All `api.beeper.com` HTTP is confined to
    /// `keeper-core`.
    pub beeper_flows: Arc<BeeperFlowRegistry>,
    /// Live archive-export jobs (Story 5.5). Maps each `exportId` to its shared
    /// `Arc<AtomicBool>` cancel flag: `export_start` registers a flag before
    /// spawning the blocking job, `export_cancel` sets it, and the job deregisters
    /// itself on any terminal phase. The `AtomicU64` mints monotonic ids.
    pub exports: Arc<ExportRegistry>,
    /// Live and recently-finished one-time copy jobs (Epic 33). A copy is a
    /// job rather than a relationship, so it lives here in app memory and
    /// never reaches `profiles` or the sync journal.
    #[cfg(desktop)]
    pub copies: Arc<crate::copy_ipc::CopyRegistry>,
    /// Live `bbctl` self-hosted-bridge runs (Story 6.7). Maps each `sessionId` to
    /// its driver-task abort handle, keyed also by `(accountId, networkId)` so a
    /// second run for the same target replaces the first rather than spawning a
    /// second unsupervised `bbctl run` daemon. `bbctl_run_start` reserves the target,
    /// spawns, and registers the handle atomically under one lock (so a fast-terminating
    /// task can never leave a resident handle); `bbctl_run_cancel` aborts and removes.
    pub bbctl_runs: Arc<BbctlRunRegistry>,
    /// When the app last reported `Background` (Story 14.4). A **wall-clock**
    /// [`SystemTime`] — never `Instant`, whose Apple `mach_absolute_time` base does not
    /// advance while the device sleeps, so an overnight suspension would read as
    /// near-zero elapsed and the matrix-rust-sdk#3935 stale-session restart would never
    /// trip. Earliest-wins: `Background` records only when this is `None` (a
    /// duplicate/late report can't shrink a long suspension); `Foreground` *takes* it.
    pub paused_at: Mutex<Option<SystemTime>>,
    /// The last phone-stack navigation level (Story 14.4) — nav *selection* only,
    /// never message/room data. Survives a jettisoned web-content process because it
    /// lives here in Rust; a true app kill starts fresh (`None` ⇒ cold launch ⇒ Inbox).
    pub nav_state: Mutex<Option<NavState>>,
    /// The `keeper-rec` sidecar port behind the recording pre-flight commands
    /// (Story 16.5). Compile-time-selected per target (see [`PlatformRecorder`]);
    /// every round-trip spawns a fresh child sidecar so TCC attributes the
    /// request to keeper (AD-36) — a persistent session is 16.6's concern.
    pub recorder: Arc<PlatformRecorder>,
    /// Whether Screen Recording has been requested this app lifetime (Story
    /// 16.5). The OS shows its one real prompt per app lifetime, so this session
    /// flag is what lifts the two-valued preflight into the honest tri-state
    /// (`notDetermined` + already-requested ⇒ denied-with-fix-path). Deliberately
    /// never persisted — a fresh session must never cache a denial (or a grant)
    /// optimistically.
    pub recording_permission_requested: AtomicBool,
    /// The (at most one) live recording session (Story 16.6): the graceful-stop
    /// trigger plus the status snapshot the driver task keeps current. `None`
    /// until the first `recording_start` of this app lifetime; a terminal
    /// session stays in the slot (its outcome is what `recording_status`
    /// reports) until the next start replaces it.
    pub recording_run: Mutex<Option<RecordingRun>>,
    /// The folders of live (or starting) recording sessions (Story 17.3): the
    /// live-session guard behind the orphan-recovery pass's `is_active`
    /// predicate. `recording_start` reserves its unique session folder here
    /// BEFORE `SessionManifest::create` (closing the create-before-
    /// `RecordingRun`-install window) and the [`LiveFolderReservation`] RAII
    /// guard removes it on every exit path — an early start error, the driver
    /// task's end after any terminal, and the quit kill-timeout's task abort.
    /// An on-disk `status:"recording"` cannot by itself distinguish a crashed
    /// orphan from a live session, so recovery skips any folder found in this
    /// set. `Arc`'d so the guard can ride into the `'static` driver task.
    pub reserved_recording_folders: Arc<Mutex<HashSet<PathBuf>>>,
    /// Serializes the two orphan-recovery call sites (Story 17.3) — the
    /// detached startup pass and the pre-record pass — so two scans never
    /// reconcile+`write` the same folder concurrently (`SessionManifest::
    /// write` uses a fixed `.manifest.json.tmp` name per folder; concurrent
    /// renames could interleave). Held around the whole scan; disjoint from
    /// the reserved-set lock, which the `is_active` predicate takes only
    /// briefly inside a scan (a consistent scan → set order, so no deadlock).
    pub recovery_scan: Mutex<()>,
}

/// The live half of one recording session (Story 16.6, AD-33): the shell owns
/// the process-facing pieces (the stop trigger and the polled status snapshot);
/// the platform-free state machine lives inside the driver task.
pub struct RecordingRun {
    /// Fires the graceful `stop` request into the session task (one-shot;
    /// `None` after a stop was requested).
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// The status snapshot shared with (and kept current by) the driver task.
    status: Arc<Mutex<RecordingStatusVm>>,
    /// The driver-task handle (Story 18.2). Aborting it drops the
    /// `run_session` future, whose `kill_on_drop(true)` child then
    /// force-terminates the `keeper-rec` sidecar — the quit kill-timeout's
    /// only force-kill lever (the child is a local inside `run_session`).
    /// `None` after [`finalize_recording_for_quit`] took it.
    driver: Option<tauri::async_runtime::JoinHandle<()>>,
    /// The segment-size cap (decimal MB) captured from the segmentation settings
    /// at `recording_start` (Story 18.3) — the segment meter's denominator,
    /// surfaced as `RecordingStatusVm::segment_cap_mb` by [`recording_snapshot`].
    /// Session-captured so a mid-session settings edit (which applies to the next
    /// session) never skews a running meter.
    segment_cap_mb: u32,
    /// The resolved destination folder this session records into (Story 18.5),
    /// captured from `effective_destination_dir` at `recording_start` — the
    /// volume the live disk-space guard probes on its ~1 Hz tick. Session-
    /// captured for the same reason as the cap: a mid-session settings edit
    /// must never repoint a running guard at a different volume.
    destination_dir: PathBuf,
    /// This session's durability reader (Story 41.6), or `None` for a
    /// plain-folder destination — which is `local` by definition and has no
    /// engine to ask. Session-captured beside the cap and the destination for
    /// the same AD-25 reason: a mid-session settings edit must not repoint a
    /// running session's durability at a profile it never wrote into.
    ///
    /// `Arc`'d because the ~1 Hz status read clones it out of this slot and
    /// then releases the lock — the slot is never held across the read, exactly
    /// as it is never held across the `read_dir`/`stat` half.
    durability: Option<Arc<RecordingDurabilityReader>>,
}

/// Lock an optional-slot mutex, recovering a poisoned lock instead of propagating —
/// these slots hold plain data (a timestamp, a nav selection) with no invariant a
/// mid-write panic could break, and a resume/nav concern must never panic the app.
pub(crate) fn slot_lock<T>(slot: &Mutex<Option<T>>) -> std::sync::MutexGuard<'_, Option<T>> {
    match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Store a value into an optional slot (poison-recovering).
pub(crate) fn slot_set<T>(slot: &Mutex<Option<T>>, value: T) {
    *slot_lock(slot) = Some(value);
}

/// Read (clone) the current value of an optional slot (poison-recovering).
pub(crate) fn slot_get<T: Clone>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot_lock(slot).clone()
}

/// Take (consume) the current value of an optional slot (poison-recovering).
pub(crate) fn slot_take<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot_lock(slot).take()
}

/// Lock any plain-data mutex, recovering a poisoned lock instead of propagating
/// — the [`slot_lock`] discipline for the non-`Option` slots (the reserved
/// live-folder set, the recovery-scan serializer): no invariant a mid-write
/// panic could break, and a best-effort recovery concern must never panic the
/// app.
fn plain_lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// RAII reservation of one live session folder in
/// [`AppState::reserved_recording_folders`] (Story 17.3): taken in
/// `recording_start` BEFORE `SessionManifest::create` — so the folder is
/// reserved for the ENTIRE span it could be live, including the
/// create→`RecordingRun`-install window — and released by `Drop` on every exit
/// path: a `?` early-return after reserving, the driver task's end after any
/// terminal (the guard rides into the task), and the quit kill-timeout's
/// `abort()` (dropping the `run_session` future drops the guard). While
/// reserved, the orphan-recovery pass's `is_active` predicate reports the
/// folder live and skips it untouched.
#[derive(Debug)]
struct LiveFolderReservation {
    reserved: Arc<Mutex<HashSet<PathBuf>>>,
    folder: PathBuf,
    /// Whether THIS guard is the one holding the entry (see [`Self::reserve`]).
    owned: bool,
}

impl LiveFolderReservation {
    /// Insert `folder` into the reserved set and return the releasing guard.
    ///
    /// Only the guard that actually INSERTED releases the entry. Story 40.3's
    /// collision retry reserves each candidate before trying it, and a candidate
    /// that turns out to be taken is frequently a folder a still-live session
    /// already reserved — an unconditional `remove` on that guard's `Drop` would
    /// un-reserve someone else's live session, and the recovery pass would then
    /// rewrite its manifest to `recovered` while it is still recording.
    fn reserve(reserved: &Arc<Mutex<HashSet<PathBuf>>>, folder: PathBuf) -> Self {
        let owned = plain_lock(reserved).insert(folder.clone());
        Self {
            reserved: reserved.clone(),
            folder,
            owned,
        }
    }

    /// Move this guard's claim from the folder it holds onto `folder`, under a
    /// single lock acquisition.
    ///
    /// Story 40.4's retitle renames the folder it claimed. The instant `rename`
    /// returns, the claim names a path the retitle no longer owns: a start that
    /// reoccupies the vacated name gets a non-owning guard (the entry is still
    /// here), and this guard's `Drop` then un-reserves that live session for the
    /// rest of its life. Taking the new claim and releasing the old one as one
    /// indivisible step is what keeps a claim held on either side of the rename
    /// with no window between them.
    fn repoint(&mut self, folder: PathBuf) {
        let owned = {
            let mut reserved = plain_lock(&self.reserved);
            let owned = reserved.insert(folder.clone());
            if self.owned {
                reserved.remove(&self.folder);
            }
            owned
        };
        self.folder = folder;
        self.owned = owned;
    }
}

impl Drop for LiveFolderReservation {
    fn drop(&mut self) {
        if self.owned {
            plain_lock(&self.reserved).remove(&self.folder);
        }
    }
}

/// The two registry maps, held under a single lock so target-reservation and
/// handle-insertion are one indivisible step (see [`BbctlRunRegistry::start`]).
#[derive(Default)]
struct BbctlRunInner {
    /// `sessionId → driver-task abort handle`.
    tasks: HashMap<u64, tokio::task::AbortHandle>,
    /// `(accountId, networkId) → sessionId` for in-flight dedupe.
    by_target: HashMap<(String, String), u64>,
}

/// The `bbctl` run registry (Story 6.7). Each in-flight run owns an entry keyed by
/// its `sessionId`, plus a `(accountId, networkId) → sessionId` index used to dedupe
/// an already-in-flight run for the same target. The `AtomicU64` mints monotonic
/// session ids.
#[derive(Default)]
pub struct BbctlRunRegistry {
    /// Monotonic session-id source.
    next_id: AtomicU64,
    /// Both maps under **one** lock so [`Self::start`] reserves the target, aborts any
    /// prior run for it, spawns, and inserts the new handle atomically.
    inner: Mutex<BbctlRunInner>,
}

impl BbctlRunRegistry {
    /// Mint a fresh session id (does not register anything — [`Self::start`] does).
    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Reserve the `(accountId, networkId)` target for `session_id`, abort any run
    /// already in flight for it, invoke `spawn` (which spawns the driver task and
    /// returns its abort handle), and register that handle — **all under one lock**.
    ///
    /// Holding the lock across reserve + spawn + insert makes those three steps
    /// indivisible, closing two races the earlier reserve-then-spawn-then-insert
    /// shape left open: (a) a racing second start for the same target always observes
    /// this run's handle in `tasks` and aborts it (true dedupe — never two daemons),
    /// and (b) a fast-terminating driver can never run [`Self::finish`] before its
    /// handle is inserted (no resident stale handle leaks). `spawn` must only
    /// `tokio::spawn` and return the handle — it must not block or await.
    fn start(
        &self,
        account_id: &str,
        network_id: &str,
        session_id: u64,
        spawn: impl FnOnce() -> tokio::task::AbortHandle,
    ) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let key = (account_id.to_owned(), network_id.to_owned());
        // Abort any prior in-flight run for the same target (replace, never a second
        // unsupervised daemon).
        if let Some(prior_id) = inner.by_target.insert(key, session_id) {
            if let Some(handle) = inner.tasks.remove(&prior_id) {
                handle.abort();
            }
        }
        let handle = spawn();
        inner.tasks.insert(session_id, handle);
    }

    /// Deregister a run on natural completion (drops its handle + target index).
    /// Idempotent — a mismatched/unknown id is a no-op.
    fn finish(&self, account_id: &str, network_id: &str, session_id: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.tasks.remove(&session_id);
        let key = (account_id.to_owned(), network_id.to_owned());
        // Only clear the index if it still points at THIS session (a newer run for
        // the same target may have replaced it).
        if inner.by_target.get(&key) == Some(&session_id) {
            inner.by_target.remove(&key);
        }
    }

    /// Cancel a run by `sessionId`: abort its driver task and remove it. Idempotent.
    fn cancel(&self, session_id: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if let Some(handle) = inner.tasks.remove(&session_id) {
            handle.abort();
        }
        inner.by_target.retain(|_, id| *id != session_id);
    }
}

/// The archive-export cancel-flag registry (Story 5.5). Each running job owns an
/// entry keyed by its `exportId`; setting the flag makes the synchronous export
/// loop stop at its next between-events check. `rusqlite` is synchronous, so a
/// drop-based cancel cannot interrupt the loop — this shared flag is how cancel
/// reaches a blocking job.
#[derive(Default)]
pub struct ExportRegistry {
    /// Monotonic export-id source.
    next_id: AtomicU64,
    /// `exportId → cancel flag`. Held under a `Mutex` since it is mutated from the
    /// command tasks and the blocking job's deregistration.
    flags: Mutex<HashMap<u64, Arc<AtomicBool>>>,
}

impl ExportRegistry {
    /// Register a fresh job: mint an id and store a cleared cancel flag. Returns the
    /// `(exportId, flag)` the caller passes into the blocking job.
    fn register(&self) -> (u64, Arc<AtomicBool>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut flags) = self.flags.lock() {
            flags.insert(id, flag.clone());
        }
        (id, flag)
    }

    /// Set the cancel flag for a job id (idempotent; a no-op for an unknown/gone id).
    fn cancel(&self, export_id: u64) {
        if let Ok(flags) = self.flags.lock() {
            if let Some(flag) = flags.get(&export_id) {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Deregister a job on any terminal phase (drops its flag). Idempotent.
    fn deregister(&self, export_id: u64) {
        if let Ok(mut flags) = self.flags.lock() {
            flags.remove(&export_id);
        }
    }
}

impl AppState {
    /// Construct the app state with the platform implementation for this build
    /// target (Story 12.2): [`DesktopPlatform`] on desktop, [`IosPlatform`] on iOS.
    ///
    /// Resolves the platform data dir up front so the [`AccountManager`] can open
    /// the single app-wide `archive.db` and spawn its serialized writer (Story
    /// 5.1). If the data dir cannot be resolved (should not happen on a supported
    /// platform), fall back to the OS temp dir for the archive path so startup still
    /// succeeds — archiving degrades rather than aborting the app.
    pub fn new() -> Self {
        #[cfg(desktop)]
        let platform: Arc<dyn Platform> = Arc::new(DesktopPlatform);
        #[cfg(target_os = "ios")]
        let platform: Arc<dyn Platform> = Arc::new(IosPlatform);
        // Story 12.2's compile seam supports desktop and iOS only. A non-iOS mobile
        // target (e.g. Android) is `mobile` — so `run()` still reaches this via the
        // `#[cfg_attr(mobile, ...)]` entry point — but binds no `platform` above.
        // Fail loudly and specifically here rather than with a bare "cannot find
        // value `platform`"; such a target needs its own `Platform` port impl.
        #[cfg(all(not(desktop), not(target_os = "ios")))]
        compile_error!(
            "no Platform implementation for this build target: Story 12.2's seam covers \
             desktop and iOS only; add a Platform port impl for other mobile targets"
        );
        let data_dir = platform.data_dir().unwrap_or_else(|e| {
            tracing::error!(error = %e, "could not resolve data dir; archive falls back to temp");
            std::env::temp_dir().join("dev.tgorka.keeper")
        });
        // The recorder shares the platform port (for sidecar resolution) on
        // desktop; iOS is the honest Unsupported impl (see `PlatformRecorder`).
        #[cfg(desktop)]
        let recorder = Arc::new(crate::recorder::DesktopRecorder::new(platform.clone()));
        #[cfg(target_os = "ios")]
        let recorder = Arc::new(crate::recorder::IosRecorder);
        Self {
            // The manager flags the `data_dir` root as backup-excluded through the
            // platform port (Story 14.7, FR-65) — best-effort, never startup-fatal.
            accounts: AccountManager::new(platform.as_ref(), &data_dir),
            platform,
            oauth_flows: Arc::new(OAuthFlowRegistry::new()),
            beeper_flows: Arc::new(BeeperFlowRegistry::new()),
            exports: Arc::new(ExportRegistry::default()),
            #[cfg(desktop)]
            copies: Arc::new(crate::copy_ipc::CopyRegistry::default()),
            bbctl_runs: Arc::new(BbctlRunRegistry::default()),
            paused_at: Mutex::new(None),
            nav_state: Mutex::new(None),
            recorder,
            recording_permission_requested: AtomicBool::new(false),
            recording_run: Mutex::new(None),
            reserved_recording_folders: Arc::new(Mutex::new(HashSet::new())),
            recovery_scan: Mutex::new(()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Monotonic source of subscription ids handed back to the frontend.
static NEXT_SUBSCRIPTION_ID: AtomicU64 = AtomicU64::new(1);

/// macOS Keychain service name under which all keeper secrets are stored (AD-3).
const KEYCHAIN_SERVICE: &str = "dev.tgorka.keeper";

/// The Tauri app handle used by the desktop `Platform::notify` port to post native
/// notifications (Story 10.1). Set exactly once in `lib.rs` `setup()` via
/// [`set_notify_app_handle`]; the write-once `OnceLock` is the one permitted global
/// (mirroring how `sidecar_path` reaches process state — `DesktopPlatform` stays a
/// unit struct). When unset (headless / CI), `notify` returns an honest `Unsupported`
/// rather than panicking.
static NOTIFY_APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Store the app handle for the desktop notifier port (Story 10.1). Called once from
/// `lib.rs` `setup()`. Idempotent — a second call is ignored (the handle is write-once).
pub fn set_notify_app_handle(handle: tauri::AppHandle) {
    let _ = NOTIFY_APP.set(handle);
}

/// The "last notification target" recorded at dispatch time (Story 10.4, Option B).
///
/// The kept `tauri-plugin-notification` desktop backend has NO per-notification click
/// callback, so exact per-notification routing is impossible on this backend (deferred to
/// Epic 11). Instead `Platform::notify` records the target of the most recently posted
/// notification here, and on the next app activation the shell emits a **coarse** navigate
/// event derived from its KIND (Message → Inbox, Bridge → Bridges). This is deliberately
/// coarse — it is NEVER exact-message routing. Guarded by a `Mutex`; the honest default is
/// [`NotifyTarget::None`] (a plain summon+focus with no view switch).
static LAST_NOTIFY_TARGET: OnceLock<Mutex<NotifyTarget>> = OnceLock::new();

fn last_notify_target_slot() -> &'static Mutex<NotifyTarget> {
    LAST_NOTIFY_TARGET.get_or_init(|| Mutex::new(NotifyTarget::None))
}

/// Record the target of the notification just posted (Story 10.4). A poisoned lock is
/// recovered rather than propagated — recording a coarse landing target must never panic.
fn record_last_notify_target(target: &NotifyTarget) {
    let mut slot = match last_notify_target_slot().lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    *slot = target.clone();
}

/// Read the "last notification target" recorded at dispatch (Story 10.4), for the coarse
/// navigate emit on app activation. A poisoned lock recovers to the stored value.
///
/// Apple-platform only: the sole caller is the `RunEvent::Reopen` (dock re-activation)
/// arm, a variant that exists nowhere else. A `desktop` gate would leave this dead on
/// the Linux/Windows shells, where `-D warnings` turns dead code into a build failure.
#[cfg(target_os = "macos")]
pub fn last_notify_target() -> NotifyTarget {
    match last_notify_target_slot().lock() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// The Tauri event the shell emits to the webview on app activation following a
/// notification (Story 10.4). Carries the recorded [`NotifyTarget`]; the frontend routes
/// its KIND to a coarse view (Message → Inbox, Bridge → Bridges). Once consumed the
/// target is reset to [`NotifyTarget::None`] so a later plain dock-click does not re-emit
/// a stale landing.
#[cfg(target_os = "macos")]
pub const NOTIFY_NAVIGATE_EVENT: &str = "notify://navigate";

/// Emit the coarse navigate event to the main window from the last recorded notification
/// target (Story 10.4, Option B), then reset the target so it fires once per notification.
///
/// A [`NotifyTarget::None`] (no notification since the last activation, e.g. a plain
/// dock-click) is a no-op — only Message/Bridge targets emit. Best-effort: a missing
/// window or an emit failure is logged at `warn`, never a panic.
#[cfg(target_os = "macos")]
pub fn emit_notify_navigate(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};

    let target = last_notify_target();
    if matches!(target, NotifyTarget::None) {
        // No pending notification landing — a plain activation. Nothing to navigate to.
        return;
    }
    // Reset so the same target is not re-emitted on a subsequent plain activation.
    record_last_notify_target(&NotifyTarget::None);

    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        tracing::warn!("notify: main window not found; cannot emit navigate event");
        return;
    };
    if let Err(error) = window.emit(NOTIFY_NAVIGATE_EVENT, &target) {
        tracing::warn!(%error, "notify: could not emit coarse navigate event");
    }
}

/// The label of the main window (matches `tauri.conf.json` / the default capability),
/// whose dock badge the desktop `Platform::set_badge_count` port drives (Story 10.3).
#[cfg(desktop)]
const MAIN_WINDOW_LABEL: &str = "main";

/// The Tauri app handle used by the desktop `Platform::set_badge_count` port to set the
/// OS dock badge on the main window (Story 10.3). Set once in `lib.rs` `setup()` via
/// [`set_badge_app_handle`]; the write-once `OnceLock` mirrors [`NOTIFY_APP`]. When unset
/// (headless / CI / before setup), `set_badge_count` is an honest no-op rather than a
/// panic — the badge computation still runs in core, it simply reaches no OS dock.
static BADGE_APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Store the app handle for the desktop dock-badge port (Story 10.3). Called once from
/// `lib.rs` `setup()`. Idempotent — a second call is ignored (the handle is write-once).
pub fn set_badge_app_handle(handle: tauri::AppHandle) {
    let _ = BADGE_APP.set(handle);
}

/// The one memo in front of this process's login keychain (Story: keychain-prompt
/// reduction).
///
/// Process-wide rather than a field on [`DesktopPlatform`] for two reasons. The
/// struct is deliberately a unit struct that reaches process state through
/// write-once globals (the same shape as [`NOTIFY_APP`] and [`BADGE_APP`]). And,
/// decisively, the shell hands out *fresh* adapters over the very same keychain —
/// `crate::sync::sync_platform` builds a new `ShellSyncPlatform` per IPC call —
/// so a per-instance memo would split one keychain across several caches with
/// several invalidation domains, and a credential corrected through one of them
/// would keep being spent by another. One keychain, one cache.
///
/// Every `Platform` keychain call in this impl goes through it: reads memoized,
/// writes and deletes invalidating. `ShellSyncPlatform`'s `secret_*` methods
/// delegate to these three, so the folder-sync engine's credential reads are
/// covered by this same instance and must NOT gain a second one.
#[cfg(desktop)]
static KEYCHAIN_CACHE: LazyLock<SecretCache> = LazyLock::new(SecretCache::new);

/// Concrete [`Platform`] implementation for the desktop shell.
///
/// The data-dir port is fully wired via `dirs`; the remaining ports return
/// [`CoreError::Unsupported`] until later stories fill them (honest, never
/// panicking).
#[cfg(desktop)]
pub struct DesktopPlatform;

#[cfg(desktop)]
impl Platform for DesktopPlatform {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        let base = dirs::data_dir().ok_or_else(|| {
            PlatformError::DirUnavailable("no OS data directory available".to_owned())
        })?;
        Ok(base.join("dev.tgorka.keeper"))
    }

    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .map_err(|e| PlatformError::Keychain(format!("could not open keychain entry: {e}")))?;
        let written = entry.set_password(value);
        // Invalidate after the attempt and regardless of its outcome: a write that
        // failed part-way may still have replaced the stored item, and continuing
        // to hand out the pre-write value would spend a credential the user has
        // already corrected — a failure strictly worse than the prompt the cache
        // removes.
        KEYCHAIN_CACHE.invalidate(key);
        written.map_err(|e| PlatformError::Keychain(format!("could not store secret: {e}")))?;
        Ok(())
    }

    /// Read a secret, reaching the OS at most once per key per process.
    ///
    /// macOS re-evaluates a keychain item's ACL on every read that *returns
    /// data*, so an unmemoized read is one "keeper wants to use your confidential
    /// information stored in dev.tgorka.keeper in your keychain" dialog **per
    /// read** until the item's ACL trusts this exact binary. That is why a machine
    /// syncing continuously kept being asked all session long: the folder-sync
    /// credential was read again on every push and every fetch.
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        KEYCHAIN_CACHE.read_through(key, || {
            let entry = keyring::Entry::new(KEYCHAIN_SERVICE, key).map_err(|e| {
                PlatformError::Keychain(format!("could not open keychain entry: {e}"))
            })?;
            match entry.get_password() {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => {
                    Err(PlatformError::Keychain(format!("could not read secret: {e}")).into())
                }
            }
        })
    }

    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .map_err(|e| PlatformError::Keychain(format!("could not open keychain entry: {e}")))?;
        let deleted = entry.delete_credential();
        // Same reasoning as `keychain_set`: drop the memo after the attempt
        // whatever it reported, so a half-failed delete cannot leave this process
        // serving a secret that is no longer there.
        KEYCHAIN_CACHE.invalidate(key);
        match deleted {
            // Deleting a missing entry is a no-op (rollback safety).
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(PlatformError::Keychain(format!("could not delete secret: {e}")).into()),
        }
    }

    fn open_url(&self, url: &str) -> Result<(), CoreError> {
        // Open in the system default browser (no explicit `with` program). Used
        // by the OIDC flow to present the OAuth authorization URL for consent.
        tauri_plugin_opener::open_url(url, None::<&str>)
            .map_err(|e| CoreError::Internal(format!("could not open the system browser: {e}")))
    }

    fn notify(&self, title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError> {
        use tauri_plugin_notification::NotificationExt;

        // Record the click-through target as the "last notification target" (Story 10.4,
        // Option B): the kept backend has no per-notification click callback, so on the
        // next app activation the shell emits a coarse navigate event derived from this
        // target's KIND (Message → Inbox, Bridge → Bridges). Recorded before the post so
        // the target is set even if the OS notifier itself errors.
        record_last_notify_target(target);

        // The app handle is set once in `setup()`; when it is unset (headless / CI)
        // this is an honest `Unsupported`, never a panic. `DesktopPlatform` stays a
        // unit struct — it reaches the handle through the write-once global.
        let app = NOTIFY_APP.get().ok_or_else(|| {
            CoreError::Unsupported("notification app handle is not set (headless)".to_owned())
        })?;
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|e| CoreError::Internal(format!("could not post notification: {e}")))
    }

    fn set_badge_count(&self, count: Option<u32>) -> Result<(), CoreError> {
        use tauri::Manager;

        // The badge app handle is set once in `setup()`; when it is unset (headless /
        // CI / pre-setup) this is an honest no-op — the badge is a comfort signal and
        // must never block or abort the inbox merge. `DesktopPlatform` stays a unit
        // struct, reaching the handle through the write-once global.
        let Some(app) = BADGE_APP.get() else {
            return Ok(());
        };
        let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
            // No main window yet (very early startup) — nothing to badge; honest no-op.
            return Ok(());
        };
        window
            .set_badge_count(count.map(i64::from))
            .map_err(|e| CoreError::Internal(format!("could not set dock badge count: {e}")))
    }

    fn exclude_from_backup(&self, _path: &Path) -> Result<(), CoreError> {
        // Per-path backup exclusion is an iOS-only concept (NSURLIsExcludedFromBackupKey);
        // desktop has no equivalent, so this is an honest no-op (Story 14.7, FR-65).
        Ok(())
    }

    fn sidecar_path(&self, name: &str) -> Result<PathBuf, CoreError> {
        // Tauri lays sidecars next to the running executable under two layouts:
        // in dev the per-arch source name keeps its target-triple suffix (e.g.
        // `keeper-rec-aarch64-apple-darwin`), while the bundler STRIPS the suffix
        // when packaging (`Contents/MacOS/keeper-rec` in the .app). Probe both —
        // triple-suffixed first (dev), bare name second (bundle). Resolve via
        // `current_exe()` — `DesktopPlatform` is a unit struct with no `AppHandle`.
        // With neither present (e.g. CI with no bundled binary) → an honest
        // `Unsupported`, which is the guided-install path (Story 6.7, AC-2),
        // never a panic.
        let exe = std::env::current_exe().map_err(|e| {
            CoreError::Unsupported(format!("could not resolve the running executable: {e}"))
        })?;
        let dir = exe.parent().ok_or_else(|| {
            CoreError::Unsupported("running executable has no parent directory".to_owned())
        })?;
        let triple = tauri::utils::platform::target_triple()
            .map_err(|e| CoreError::Unsupported(format!("could not resolve target triple: {e}")))?;
        for base in [format!("{name}-{triple}"), name.to_owned()] {
            let mut candidate = dir.join(base);
            if cfg!(target_os = "windows") {
                candidate.set_extension("exe");
            }
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(CoreError::Unsupported(format!(
            "sidecar {name:?} not found next to the executable"
        )))
    }
}

/// Concrete [`Platform`] implementation for the iOS shell (Story 12.2).
///
/// The Apple-shared ports (data dir via `dirs`, keychain via `keyring`'s
/// `apple-native` backend, browser-open via the opener plugin, notifications via
/// the notification plugin) mirror the desktop bodies. The desktop-only ports are
/// honest about their absence: `sidecar_path` returns [`CoreError::Unsupported`]
/// (no child processes / sidecars on iOS — ever), and `set_badge_count` is a
/// no-op (the desktop dock badge does not exist; an iOS app badge arrives with
/// the push/notification work in a later story, never here).
#[cfg(target_os = "ios")]
pub struct IosPlatform;

/// `errSecItemNotFound` (`-25300`) — the Security Framework status returned when no
/// keychain item matches. `security_framework` does not re-export it, and although
/// `security-framework-sys` (which does) is already in the tree transitively, using its
/// constant would mean declaring a direct `-sys` dependency. Since this is a stable,
/// ABI-fixed Apple `OSStatus`, a local `const` is safe; `Error::code()` yields an `i32`,
/// so the comparison is a plain numeric match (Story 12.3, AD-29).
#[cfg(target_os = "ios")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_os = "ios")]
impl Platform for IosPlatform {
    /// The single app-container root for all account state on iOS.
    ///
    /// Inside the iOS sandbox `dirs::data_dir()` resolves to the app container's
    /// `Library/Application Support`, so this returns
    /// `{container}/Library/Application Support/dev.tgorka.keeper` — the one root under
    /// which `accounts/<ulid>/sdk`, `keeper.db`, and `archive.db` all live (Story 12.3).
    /// A future App Group move relocates this single path; it is a path change, not a
    /// data migration (`NSFileProtection*` / `isExcludedFromBackup` are Epic 14 / 14.7).
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        let base = dirs::data_dir().ok_or_else(|| {
            PlatformError::DirUnavailable("no OS data directory available".to_owned())
        })?;
        Ok(base.join("dev.tgorka.keeper"))
    }

    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        use security_framework::access_control::{ProtectionMode, SecAccessControl};
        use security_framework::passwords::{
            delete_generic_password, set_generic_password_options, PasswordOptions,
        };

        // Pin the item to `AfterFirstUnlockThisDeviceOnly` via a protection-only
        // access control (flags = 0, no biometry/passcode/user-presence): readable
        // headless by the resumed sync loop, never iCloud-synced, invisible to other
        // apps (Story 12.3, AD-29).
        let access_control = SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly),
            0,
        )
        .map_err(|e| PlatformError::Keychain(format!("could not build access control: {e}")))?;
        let mut options = PasswordOptions::new_generic_password(KEYCHAIN_SERVICE, key);
        options.set_access_control(access_control);
        // Delete any prior item so the fresh `SecItemAdd` carries the protection class
        // (an update whose match query carries `kSecAttrAccessControl` is fragile). A
        // missing item is fine — this is a best-effort clear before the authoritative add.
        // The two calls are not atomic, but keychain keys here are write-once per account
        // (`session/<ulid>`, `store_passphrase/<ulid>`); a failure between them surfaces as
        // an `Err` and degrades to re-login (a session-less account is skipped on restore),
        // never silent corruption.
        let _ = delete_generic_password(KEYCHAIN_SERVICE, key);
        set_generic_password_options(value.as_bytes(), options)
            .map_err(|e| PlatformError::Keychain(format!("could not store secret: {e}")))?;
        Ok(())
    }

    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        use security_framework::passwords::get_generic_password;

        match get_generic_password(KEYCHAIN_SERVICE, key) {
            Ok(bytes) => {
                let secret = String::from_utf8(bytes).map_err(|e| {
                    PlatformError::Keychain(format!("stored secret was not valid UTF-8: {e}"))
                })?;
                Ok(Some(secret))
            }
            // A missing item is `Ok(None)`, not an error (accessibility is not part of
            // the match query, so an AC-protected item is still found with no prompt).
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(PlatformError::Keychain(format!("could not read secret: {e}")).into()),
        }
    }

    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        use security_framework::passwords::delete_generic_password;

        match delete_generic_password(KEYCHAIN_SERVICE, key) {
            Ok(()) => Ok(()),
            // Deleting a missing entry is a no-op (rollback safety).
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(PlatformError::Keychain(format!("could not delete secret: {e}")).into()),
        }
    }

    fn open_url(&self, url: &str) -> Result<(), CoreError> {
        // "Open in browser" stays `tauri_plugin_opener::open_url` on iOS too —
        // it hands the URL to the OS (Safari / the default handler).
        tauri_plugin_opener::open_url(url, None::<&str>)
            .map_err(|e| CoreError::Internal(format!("could not open the system browser: {e}")))
    }

    fn notify(&self, title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError> {
        use tauri_plugin_notification::NotificationExt;

        // Mirror the desktop port: record the coarse click-through target, then
        // post through the (mobile-capable) notification plugin. When the handle
        // is unset this is an honest `Unsupported`, never a panic.
        record_last_notify_target(target);
        let app = NOTIFY_APP.get().ok_or_else(|| {
            CoreError::Unsupported("notification app handle is not set (headless)".to_owned())
        })?;
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|e| CoreError::Internal(format!("could not post notification: {e}")))
    }

    fn set_badge_count(&self, count: Option<u32>) -> Result<(), CoreError> {
        // Story 14.3 fix (FR-62): `WebviewWindow::set_badge_count` is `#[cfg(desktop)]`
        // in Tauri and does not exist on iOS (found by Story 15.4's compile gate).
        // Use `UNUserNotificationCenter::setBadgeCount` instead — the modern iOS 16+
        // API, and a SAFE binding in objc2-user-notifications (no unsafe block).
        // `None`/0 clears the badge. Best-effort by design: the completion handler is
        // omitted, so a runtime refusal (e.g. badge permission denied) is silently
        // ignored — the badge is a comfort signal and must never fail the caller.
        use objc2_user_notifications::UNUserNotificationCenter;

        let center = UNUserNotificationCenter::currentNotificationCenter();
        let value = count.map_or(0isize, |c| isize::try_from(c).unwrap_or(isize::MAX));
        center.setBadgeCount_withCompletionHandler(value, None);
        Ok(())
    }

    /// Exclude `path` from iCloud/iTunes device backups by setting
    /// `NSURLIsExcludedFromBackupKey` on its file URL (Story 14.7, FR-65).
    ///
    /// This is the codebase's single authorized `unsafe` FFI: `isExcludedFromBackup`
    /// has no safe binding, so the setter is reached through objc2-foundation behind
    /// this port — function-level `#[allow(unsafe_code)]`, `// SAFETY:`-documented,
    /// and listed in the audit inventory in `docs/constraints-and-limitations.md`
    /// (coordinator policy amendment, 2026-07-11). Directory-level exclusion covers
    /// the whole subtree, which is how each store's SQLite `-wal`/`-shm` sidecars
    /// are kept out of backup. Precondition: callers pass absolute, already-created
    /// **directories** rooted under `data_dir` (the `data_dir` root and each
    /// `accounts/<ulid>/sdk`), hence `fileURLWithPath_isDirectory` with
    /// `is_directory = true` — no extra stat.
    #[allow(unsafe_code)]
    fn exclude_from_backup(&self, path: &Path) -> Result<(), CoreError> {
        // objc2 types are used inside the method body only, so no iOS-only import
        // leaks to the desktop compile (mirrors the 12.3 keychain pattern).
        use objc2_foundation::{NSNumber, NSString, NSURLIsExcludedFromBackupKey, NSURL};

        // App-container paths are ASCII in practice; a non-UTF-8 path is near-
        // unreachable and surfaces as an Err the (best-effort) callers log.
        let path_str = path.to_str().ok_or_else(|| {
            PlatformError::BackupExclusion(format!("path is not valid UTF-8: {}", path.display()))
        })?;
        let ns_path = NSString::from_str(path_str);
        let url = NSURL::fileURLWithPath_isDirectory(&ns_path, true);
        let value = NSNumber::new_bool(true);
        // SAFETY: `NSURLIsExcludedFromBackupKey` is Apple's documented, process-
        // lifetime `NSURLResourceKey` extern static — reading it carries no other
        // obligation. `setResourceValue:forKey:error:` requires a valid file URL
        // and a value of the key's documented type: `url` is a file URL built just
        // above, `value` is the boolean `NSNumber` the key documents, and both are
        // owned `Retained` references that outlive the call. The setter only
        // writes the URL's resource cache + the path's extended attribute; a
        // runtime failure (e.g. the path does not exist) is returned as
        // `Err(NSError)`, never undefined behavior.
        let result = unsafe {
            url.setResourceValue_forKey_error(Some(&value), NSURLIsExcludedFromBackupKey)
        };
        result.map_err(|e| {
            PlatformError::BackupExclusion(format!(
                "could not set NSURLIsExcludedFromBackupKey on {}: {e}",
                path.display()
            ))
            .into()
        })
    }

    fn sidecar_path(&self, name: &str) -> Result<PathBuf, CoreError> {
        // No child processes / sidecars on iOS, ever (Story 12.2 boundary). The
        // `Unsupported` funnels through `to_ipc_error` to
        // `IpcErrorCode::Unsupported` (`retriable: false`) at the command edge.
        Err(CoreError::Unsupported(format!(
            "sidecar {name:?} is not available on iOS"
        )))
    }
}

/// The logical sidecar name for the Beeper `bbctl` CLI (Story 6.7). Resolved per-arch
/// next to the executable via [`Platform::sidecar_path`].
const BBCTL_SIDECAR_NAME: &str = "bbctl";

/// The desktop [`BbctlRunner`] (Story 6.7, FR-29). `is_available` is simply whether
/// the `bbctl` sidecar resolves; `run` spawns it via `tokio::process` on the resolved
/// path — no `tauri-plugin-shell`, no `externalBin`, no new capability.
///
/// The runner **pipes AND reads BOTH stdout and stderr** (bbctl is a Go CLI that logs
/// progress/markers to stderr), merging their lines through `on_line`. It honors an
/// `on_line` `Stop` by ending the read promptly and returning
/// [`BbctlRunExit::StoppedEarly`] — it does NOT `child.wait()` and does NOT kill the
/// child (a `bbctl run` daemon keeps running, launch-and-leave). A single non-UTF-8
/// line is skipped (NOT treated as clean EOF), and the reader keeps going.
/// Aborts the wrapped task when dropped. Wraps the `bbctl` stdout/stderr reader
/// tasks so they are torn down whenever the `run` future is dropped — including a
/// `bbctl_run_cancel` that aborts the driver task mid-stream — leaving no reader
/// task or pipe fd leaked. The launched `bbctl run` daemon itself is untouched
/// (launch-and-leave); only keeper's readers stop.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct DesktopBbctlRunner {
    platform: Arc<dyn Platform>,
}

impl DesktopBbctlRunner {
    /// Construct a runner sharing the app's platform port (for sidecar resolution).
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }
}

impl keeper_core::bridges::bbctl::BbctlRunner for DesktopBbctlRunner {
    fn is_available(&self) -> bool {
        self.platform.sidecar_path(BBCTL_SIDECAR_NAME).is_ok()
    }

    async fn run(
        &self,
        args: Vec<String>,
        mut on_line: Box<dyn FnMut(&str) -> keeper_core::bridges::bbctl::LineControl + Send>,
    ) -> Result<keeper_core::bridges::bbctl::BbctlRunExit, BridgeError> {
        use keeper_core::bridges::bbctl::{BbctlRunExit, LineControl};
        use tokio::io::{AsyncBufReadExt, BufReader};

        let path = self
            .platform
            .sidecar_path(BBCTL_SIDECAR_NAME)
            .map_err(|e| BridgeError::Bbctl(format!("bbctl is unavailable: {e}")))?;

        let mut child = tokio::process::Command::new(&path)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| BridgeError::Bbctl(format!("could not launch bbctl: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BridgeError::Bbctl("could not capture bbctl stdout".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BridgeError::Bbctl("could not capture bbctl stderr".to_owned()))?;

        // Merge stdout + stderr lines onto one channel so a single `on_line` loop
        // sees both streams in arrival order. Each reader task streams `Vec<u8>`
        // lines (byte-level so a non-UTF-8 line is skipped, never a false EOF).
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let out_tx = tx.clone();
        // Wrapped in `AbortOnDrop` so the readers are torn down whenever this `run`
        // future is dropped (early stop OR a driver-cancel), never leaking.
        let _out_reader = AbortOnDrop(tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if out_tx.send(buf.clone()).is_err() {
                            break;
                        }
                    }
                    // A read error ends this stream only — never treated as the
                    // whole run's clean EOF.
                    Err(_) => break,
                }
            }
        }));
        let _err_reader = AbortOnDrop(tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(buf.clone()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }));

        // Consume merged lines. A `Stop` resolves `StoppedEarly` immediately —
        // WITHOUT `child.wait()` and WITHOUT killing the child (launch-and-leave).
        let mut early_stop = false;
        while let Some(raw) = rx.recv().await {
            // Decode lossily; a non-UTF-8 line is not an EOF — we still get a line
            // (replacement chars) and keep reading.
            let line = String::from_utf8_lossy(&raw);
            let trimmed = line.trim_end_matches(['\n', '\r']);
            if trimmed.is_empty() {
                continue;
            }
            if on_line(trimmed) == LineControl::Stop {
                early_stop = true;
                break;
            }
        }

        if early_stop {
            // Leave the child running; the reader tasks are aborted when their
            // `AbortOnDrop` guards drop at scope exit. A `bbctl_run_cancel` that
            // aborts the driver task mid-stream drops this whole future — and with
            // it the guards — so the readers never leak either.
            return Ok(BbctlRunExit::StoppedEarly);
        }

        // Both streams reached EOF (the process is exiting) — reap the status.
        let status = child
            .wait()
            .await
            .map_err(|e| BridgeError::Bbctl(format!("bbctl did not exit cleanly: {e}")))?;
        Ok(BbctlRunExit::Exited(status.code().unwrap_or(-1)))
    }
}

/// The single `CoreError -> IpcError` mapping (AD-21). Every fallible command
/// funnels its errors through here exactly once.
pub(crate) fn to_ipc_error(err: CoreError) -> IpcError {
    let (code, retriable) = match &err {
        CoreError::Platform(PlatformError::Unsupported(_)) | CoreError::Unsupported(_) => {
            (IpcErrorCode::Unsupported, false)
        }
        CoreError::Platform(PlatformError::DirUnavailable(_)) => (IpcErrorCode::Internal, false),
        CoreError::Platform(PlatformError::Keychain(_)) => (IpcErrorCode::Internal, false),
        // Story 14.7: backup exclusion is best-effort hardening — keeper-core logs and
        // swallows it at every call site, so it should never reach a command edge. If it
        // ever does, it is an internal, non-retriable condition (mirrors Keychain).
        CoreError::Platform(PlatformError::BackupExclusion(_)) => (IpcErrorCode::Internal, false),
        CoreError::Internal(_) => (IpcErrorCode::Internal, false),
        CoreError::Auth(AuthError::ServerUnreachable(_)) => (IpcErrorCode::ServerUnreachable, true),
        CoreError::Auth(AuthError::InvalidCredentials) => (IpcErrorCode::InvalidCredentials, false),
        CoreError::Auth(AuthError::UnsupportedLoginType(_)) => {
            (IpcErrorCode::UnsupportedLoginType, false)
        }
        CoreError::Auth(AuthError::SlidingSyncUnsupported) => {
            (IpcErrorCode::SlidingSyncUnsupported, false)
        }
        // OIDC not offered by the homeserver: nothing to retry — the user must
        // pick a different login mechanism.
        CoreError::Auth(AuthError::OAuthUnsupported) => (IpcErrorCode::OauthUnsupported, false),
        // A cancelled / timed-out / failed OIDC flow is retriable: the user can
        // start the browser sign-in again.
        CoreError::Auth(AuthError::OAuthCancelled) => (IpcErrorCode::OauthCancelled, true),
        CoreError::Auth(AuthError::OAuthTimedOut) => (IpcErrorCode::OauthTimedOut, true),
        CoreError::Auth(AuthError::OAuthFailed(_)) => (IpcErrorCode::OauthFailed, true),
        // Every Beeper failure (non-2xx / timeout / transport / shape change /
        // abandoned flow / JWT-login rejection) collapses to this one retriable
        // code: the UI returns to the email step to start a fresh flow.
        CoreError::Auth(AuthError::BeeperUnavailable(_)) => (IpcErrorCode::BeeperUnavailable, true),
        // Any account activation / sync-start failure is retriable: the
        // frontend can attempt the subscribe again.
        CoreError::Account(
            AccountError::SessionMissing
            | AccountError::RestoreFailed(_)
            | AccountError::SyncStart(_),
        ) => (IpcErrorCode::SyncUnavailable, true),
        // A merged-inbox stream start failure is retriable: the frontend can
        // re-subscribe the inbox.
        CoreError::Inbox(InboxError::StreamStart(_)) => (IpcErrorCode::SyncUnavailable, true),
        // A room-not-found or timeline-build failure is retriable: the frontend
        // can attempt the subscribe again.
        CoreError::Timeline(TimelineError::RoomNotFound | TimelineError::Build(_)) => {
            (IpcErrorCode::TimelineUnavailable, true)
        }
        // Any enqueue-time send failure is retriable: the frontend can attempt
        // the send/retry again. Asynchronous delivery failures never reach here —
        // they surface as the `Failed` send-state on the timeline item.
        CoreError::Send(
            SendError::RoomNotFound
            | SendError::NoOpenTimeline
            | SendError::EchoNotFound
            | SendError::Dispatch(_)
            | SendError::Upload(_),
        ) => (IpcErrorCode::SendFailed, true),
        // A reply/edit target that isn't in the live timeline, an edit of a
        // non-own/non-text message, or an approve of an empty draft is *not*
        // retriable — re-issuing the same request won't help (Story 3.4, 7.3).
        // Same `SendFailed` code, `false`. The empty-body guard exists so the
        // frontend's catch retains the draft rather than clearing unsent text.
        CoreError::Send(
            SendError::TargetNotFound | SendError::NotEditable | SendError::EmptyBody,
        ) => (IpcErrorCode::SendFailed, false),
        // Any verification failure (crypto not ready / flow not found / SDK action
        // failure) is retriable: the user can restart verification.
        CoreError::Verification(
            VerificationError::Unavailable(_)
            | VerificationError::FlowNotFound
            | VerificationError::Action(_),
        ) => (IpcErrorCode::VerificationFailed, true),
        // Key-backup errors carry *named* codes so an invalid recovery key is
        // never a generic failure (FR-14): a malformed key and a
        // well-formed-but-wrong key are distinguished, and an existing-backup
        // race offers restore. All are retriable — the user can try again.
        CoreError::Backup(BackupError::MalformedRecoveryKey) => {
            (IpcErrorCode::BackupMalformedKey, true)
        }
        CoreError::Backup(BackupError::IncorrectRecoveryKey) => {
            (IpcErrorCode::BackupIncorrectKey, true)
        }
        CoreError::Backup(BackupError::AlreadyExistsOnServer) => (IpcErrorCode::BackupExists, true),
        CoreError::Backup(
            BackupError::Unavailable(_) | BackupError::RestoreFailed(_) | BackupError::Action(_),
        ) => (IpcErrorCode::BackupFailed, true),
        // A best-effort receipt/typing signal dispatch failure (Story 3.9, AD-14).
        // In practice receipts/typing are swallowed in the core (never surfaced),
        // so this arm keeps the funnel exhaustive; if one ever surfaces it is a
        // non-retriable, best-effort signal failure.
        CoreError::Signal(SignalError::Dispatch(_)) => (IpcErrorCode::SignalDispatchFailed, false),
        // Media resolution/fetch errors never reach the IPC command surface —
        // decrypted bytes travel only over the `keeper-media://` protocol, which
        // maps these to HTTP status codes itself (Story 3.6, AD-4). This arm keeps
        // the funnel exhaustive; a media failure is an internal, non-retriable IPC
        // error should one ever surface here.
        CoreError::Media(MediaError::NotFound | MediaError::Fetch(_)) => {
            (IpcErrorCode::Internal, false)
        }
        // Archive Sqlite/serialization errors (Story 5.1) surface only at archive
        // setup and never cross the IPC command surface — a runtime write failure is
        // swallowed inside the writer task. This arm keeps the funnel exhaustive: an
        // internal, non-retriable IPC error should one ever reach here.
        CoreError::Archive(ArchiveError::Sqlite(_) | ArchiveError::Serialization(_)) => {
            (IpcErrorCode::Internal, false)
        }
        // An export IO failure (Story 5.5) — e.g. a read-only destination folder — is
        // surfaced to the export UI's persistent alert. Marked retriable: the user
        // can pick a writable destination and start the export again. (Terminal
        // export failures are normally streamed on the `Failed` batch; this arm
        // covers the `export_start`-time / synchronous-setup path.)
        CoreError::Archive(ArchiveError::ExportIo(_)) => (IpcErrorCode::Internal, true),
        // A malformed embedded bridge data file (Story 6.1) is an internal invariant
        // violation, not a user-actionable retry — the JSON is compiled in. The
        // Bridges view shows an error state and there is nothing to retry.
        CoreError::Bridge(BridgeError::Data(_)) => (IpcErrorCode::Internal, false),
        // Bridge discovery (Story 6.2) against an account that is not live — the
        // account must be activated first. Not user-actionable as a retry.
        CoreError::Bridge(BridgeError::AccountNotFound(_)) => (IpcErrorCode::Internal, false),
        // A total bridge-discovery transport failure (Story 6.2) — the homeserver
        // may be transiently unreachable. Retriable: the Bridges view can retry.
        CoreError::Bridge(BridgeError::Discovery(_)) => (IpcErrorCode::SyncUnavailable, true),
        // A native bridge-login provisioning failure (Story 6.3) — the bridge
        // returned an error, no provisioning API was reachable, or a step failed.
        // Retriable: the login Sheet offers Retry. The message is the bridge's own
        // verbatim text.
        CoreError::Bridge(BridgeError::Provisioning(_)) => (IpcErrorCode::SyncUnavailable, true),
        // A Bridge Bot fallback-login failure (Story 6.4) — the bot didn't respond,
        // its reply couldn't be classified, or the bot DM couldn't be resolved.
        // Retriable, mirroring the provisioning arm: the login Sheet offers Retry and
        // the message is the bot's own verbatim text.
        CoreError::Bridge(BridgeError::Bot(_)) => (IpcErrorCode::SyncUnavailable, true),
        // A bbctl self-hosted-bridge run failure or refusal (Story 6.7) — a
        // non-Beeper gate, an unsupported network, an absent sidecar, or a bbctl
        // process error. Retriable: the run Sheet offers Retry. The message is
        // bbctl's own verbatim text (or keeper's honest gate/install reason).
        CoreError::Bridge(BridgeError::Bbctl(_)) => (IpcErrorCode::SyncUnavailable, true),
        // A rejected recording destination (Story 19.5) — the validate-on-Start
        // pre-flight blocked capture before any session folder or sidecar
        // existed. Retriable, mirroring the `ExportIo` destination arm: the user
        // can free space or choose another folder and press Start again. The
        // message is the rejection's actionable, secret-free `Display`.
        CoreError::Recording(RecordingError::DestinationInvalid { .. }) => {
            (IpcErrorCode::Internal, true)
        }
        // A rejected recording path template (Story 40.2). NOT retriable: the
        // request carried a template that cannot parse, so resubmitting it can
        // only fail identically — the user edits the field, and the message is
        // the parse reason telling them what to edit. Its own code rather than
        // `internal` for the reason `NotesInvalid` carries: the input is what is
        // wrong, and a surface that can name the fault must be able to tell this
        // apart from a backend failure it should not blame the user for.
        CoreError::Recording(RecordingError::TemplateInvalid { .. }) => {
            (IpcErrorCode::RecordingTemplateInvalid, false)
        }
        // Other recording errors (Story 16.2) do not cross the IPC command surface
        // in this story — the recording session machine and its `keeper-rec` port
        // are driven shell-side, not from a command. This arm keeps the funnel
        // exhaustive; a dedicated recording IPC surface (with its own honest codes)
        // arrives in a later recording story (16.3+). Until then, an internal,
        // non-retriable error.
        CoreError::Recording(_) => (IpcErrorCode::Internal, false),
        // Notes (Phase 5). The split is the one the phase's build contract fixes,
        // and it is a real distinction rather than a formality: a malformed name,
        // space query, template or frontmatter block is something the CALLER can
        // fix, so it is `InvalidInput` and the surface can say what to change. A
        // missing note or an unknown vault is not — by the time a notes command
        // runs, both ids came from a view model keeper itself produced, so either
        // one means the index and the caller disagree, which is internal.
        CoreError::Notes(
            NotesError::Frontmatter { .. }
            | NotesError::Name(_)
            | NotesError::Query { .. }
            | NotesError::Template(_),
        ) => (IpcErrorCode::NotesInvalid, false),
        CoreError::Notes(NotesError::NotFound(_) | NotesError::VaultUnknown(_)) => {
            (IpcErrorCode::Internal, false)
        }
    };
    IpcError {
        code,
        message: err.to_string(),
        account_id: None,
        retriable,
    }
}

/// Run a blocking body on the runtime's blocking pool and await it (AD-34-5).
///
/// Tauri v2 invokes a non-`async` `#[tauri::command]` on the main thread, which on
/// macOS is the same thread `startDragging` resolves to `performWindowDragWithEvent`
/// on — so a `read_dir`/`stat`/`keeper.db` read there is a window that will not
/// move while it runs. Marking the command `async` gets it off the main thread, but
/// it then occupies a runtime worker for the duration, which is what starves
/// messaging (the same reason [`export_start`] hands its job to
/// `tokio::task::spawn_blocking`). Both hazards go away by handing the synchronous
/// body to the blocking pool.
///
/// A join failure means the body panicked or the runtime is shutting down; neither
/// is retriable, so it funnels through [`to_ipc_error`] as `Internal`.
async fn off_async_runtime<T, F>(body: F) -> Result<T, IpcError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(body).await.map_err(|error| {
        to_ipc_error(CoreError::Internal(format!(
            "a blocking command task failed: {error}"
        )))
    })
}

/// Read a required raw-string request header (ASCII value), mapping a missing /
/// non-ASCII value to a retriable `SendFailed` IPC error. Used by the raw-body
/// pasted-attachment command for `accountId`/`roomId`/`mime` (all ASCII).
fn required_header(headers: &tauri::http::HeaderMap, name: &str) -> Result<String, IpcError> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            to_ipc_error(CoreError::Send(SendError::Upload(format!(
                "pasted attachment is missing the `{name}` header"
            ))))
        })
}

/// Read an optional percent-encoded request header and decode it back to a UTF-8
/// string (`None` when absent or malformed). Used for `filename`/`caption`, which
/// may contain non-ASCII that an ASCII-only header value cannot carry verbatim.
fn decode_header(headers: &tauri::http::HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(name)?.to_str().ok()?;
    percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .ok()
        .map(|cow| cow.into_owned())
        .filter(|s| !s.is_empty())
}

/// Current wall-clock time in milliseconds since the Unix epoch (UTC).
///
/// A skewed clock is clamped (never panics), but the anomaly is surfaced via
/// `tracing` rather than swallowed — a silently-wrong timestamp is a debugging
/// trap for later timeline-ordering stories that consume `ts`.
fn now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or_else(|_| {
            tracing::warn!("system clock beyond i64::MAX ms; clamping timestamp to i64::MAX");
            i64::MAX
        }),
        Err(_) => {
            tracing::warn!("system clock is before the Unix epoch; clamping timestamp to 0");
            0
        }
    }
}

/// Liveness command — resolves to a [`PingVm`].
///
/// Exercises the [`Platform`] port end-to-end by resolving the data directory
/// through the injected implementation, proving the platform-free seam.
#[tauri::command]
pub fn app_ping(state: State<'_, AppState>) -> Result<PingVm, IpcError> {
    // Resolve the data dir through the port to prove the seam; discard the
    // path (Story 1.1 does not create it yet).
    let _data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    Ok(PingVm {
        message: "pong".to_owned(),
        ts: now_ms(),
    })
}

/// The per-platform capability handshake (Story 12.2): the flat, data-driven
/// [`CapabilitiesVm`] the frontend mirrors at startup so it never consults user
/// agents or build flags. `false` means the surface is absent on this build.
///
/// Populated here — the shell is the platform adapter layer — with `cfg!(desktop)`
/// so `keeper-core` stays free of `cfg(target_os)` (AD-26). A later target
/// (Android / Windows) reuses the mechanism by reporting its own flags.
#[tauri::command]
pub fn capabilities(state: State<'_, AppState>) -> Result<CapabilitiesVm, IpcError> {
    Ok(CapabilitiesVm {
        tray_icon: cfg!(desktop),
        global_hotkey: cfg!(desktop),
        launch_at_login: cfg!(desktop),
        in_app_updater: cfg!(desktop),
        native_menu_bar: cfg!(desktop),
        bridge_sidecar: cfg!(desktop),
        reveal_in_file_manager: cfg!(desktop),
        // Screen recording (Story 16.3) is desktop macOS ≥ 13.0 only — a runtime
        // OS-version probe in the shell adapter (AD-35), not a bare `cfg!(desktop)`.
        // Any detection failure defaults to `false` (safe-hide).
        recording: crate::macos_version::recording_supported(),
        // Folder sync (Story 23.5, AD-41/AD-51) needs a `git` binary that clears
        // the engine's version floor. Derived from the same resolution
        // [`sync_git_status`] reports (Story 34.14), so the capability cannot say
        // yes where `Engine::open` says no: before that it asked only whether a
        // file called `git` existed, and a machine whose first `PATH` git was
        // 2.23 got a full Sync surface over an engine that had refused to open.
        sync: git_report(&state).state == SyncGitState::Ok,
        // The overlay title bar (Story 34.2) is a pure platform fact, not a probe:
        // `titleBarStyle`/`hiddenTitle` in `tauri.conf.json` are macOS-only keys, so
        // only a desktop macOS build floats the window controls over the webview and
        // needs the app to supply its own drag region and traffic-light clearance.
        overlay_title_bar: cfg!(all(desktop, target_os = "macos")),
        // Notes (Story 35.2, FR-122, AD-54): a vault IS a synced folder, so notes
        // cannot be available where folder sync is not. `sync && desktop` — which
        // on iOS is `false` twice over, and the whole surface is then absent
        // rather than disabled (AD-27).
        notes: notes_available(&state),
        // Sessions (FR-223, AD-107): the same construction as notes — a sessions
        // root IS a synced folder plus a flag, so the capability is exactly the
        // notes capability's condition, computed once and shared.
        sessions: notes_available(&state),
        // Bots (Epic 61, FR-378): `cfg!(desktop)` and **deliberately not**
        // `notes_available`. Every other surface in this struct that rides the
        // sync gate does so because its record IS a synced folder; a
        // conversation is not. A provider is a URL and a credential behind the
        // secret port, and a conversation is two tables in `keeper.db` — the
        // same database the account registry lives in — so nothing here needs
        // `git` and nothing here reads `sync.db`. Gating on `sessions` would
        // hide a working surface on every desktop whose `git` is older than the
        // engine's floor, which is a dishonest absence rather than an honest
        // one.
        //
        // A bare `cfg!` rather than a probe because there is nothing to probe:
        // the surface needs no binary, no OS version and no database that might
        // not open. The half that DOES need `sync` is the drive-tool grant
        // (Stories 61.10, 61.11), and that affordance reads
        // `CapabilitiesVm.sync` where it is offered rather than narrowing this
        // flag — two facts, two flags.
        bots: cfg!(desktop),
    })
}

/// Whether this build and this machine can show notes (FR-122).
///
/// Derived from the same `git` resolution `CapabilitiesVm.sync` reports, because a
/// vault is a folder keeper syncs: a machine whose `git` the engine refuses has no
/// sync surface and must not get a notes surface over an engine that never opened.
pub(crate) fn notes_available(state: &AppState) -> bool {
    cfg!(desktop) && git_report(state).state == SyncGitState::Ok
}

/// The same answer for a caller that holds an app handle rather than the state —
/// the tray, which decides at menu-build time whether the notes section exists at
/// all (AD-61: a section omitted from the first menu can never appear).
#[cfg(desktop)]
pub fn notes_capability(app: &tauri::AppHandle) -> bool {
    use tauri::Manager as _;

    notes_available(&app.state::<AppState>())
}

/// The mobile twins of the four desktop-only notes commands (AD-27, AD-33).
///
/// They live here rather than in `notes_ipc`, because that module is
/// `#[cfg(desktop)]` in full — it links `keeper-sync`, which iOS must never pull —
/// so a twin inside it would not exist on the target it exists for. This is where
/// every other mobile stub in the shell already lives (`reveal_path`,
/// `hotkey_get`, `launch_at_login_get`), and keeping them together is what makes
/// the `invoke_handler` list identical on every target: a phone gets an honest
/// `unsupported` rather than an `invoke` rejection the frontend would have to
/// special-case.
///
/// The rest of the notes surface is absent on iOS by construction rather than by
/// stub: `CapabilitiesVm.notes` is `false` there, so no notes surface renders and
/// nothing calls the other commands (FR-122).
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_capture_show() -> Result<(), IpcError> {
    Err(to_ipc_error(CoreError::Unsupported(
        "the quick-capture panel is desktop-only".to_owned(),
    )))
}

/// Mobile twin of `notes_capture_hide`.
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_capture_hide() -> Result<(), IpcError> {
    Err(to_ipc_error(CoreError::Unsupported(
        "the quick-capture panel is desktop-only".to_owned(),
    )))
}

/// Mobile twin of `notes_capture_open` (Story 45.15).
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_capture_open(target: keeper_core::capture::CaptureTargetVm) -> Result<(), IpcError> {
    let _ = target;
    Err(to_ipc_error(CoreError::Unsupported(
        "the quick-capture panel is desktop-only".to_owned(),
    )))
}

/// Mobile twin of `notes_capture_close` (Story 45.15).
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_capture_close(key: String) -> Result<(), IpcError> {
    let _ = key;
    Err(to_ipc_error(CoreError::Unsupported(
        "the quick-capture panel is desktop-only".to_owned(),
    )))
}

/// Mobile twin of `notes_capture_set_locked` (Story 45.15).
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_capture_set_locked(key: String, locked: bool) -> Result<(), IpcError> {
    let _ = (key, locked);
    Err(to_ipc_error(CoreError::Unsupported(
        "the quick-capture panel is desktop-only".to_owned(),
    )))
}

/// Mobile twin of `notes_capture_set_always_on_top` (Story 48.4).
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_capture_set_always_on_top(key: String, always_on_top: bool) -> Result<(), IpcError> {
    let _ = (key, always_on_top);
    Err(to_ipc_error(CoreError::Unsupported(
        "the quick-capture panel is desktop-only".to_owned(),
    )))
}

/// Mobile twin of `notes_capture_windows` (Story 45.15).
///
/// An empty list rather than a refusal, and that is the one twin here that is
/// not an error: "which capture windows are open?" has a true answer on a
/// phone — none — and a surface that asks in order to decide whether to offer
/// something should get it rather than an exception to swallow.
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_capture_windows() -> Result<Vec<keeper_core::capture::CaptureWindowVm>, IpcError> {
    Ok(Vec::new())
}

/// Mobile twin of `notes_reveal`.
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_reveal(vault_id: String, note_id: String) -> Result<(), IpcError> {
    let _ = (vault_id, note_id);
    Err(to_ipc_error(CoreError::Unsupported(
        "revealing a note in the file manager is desktop-only".to_owned(),
    )))
}

/// Mobile twin of `notes_open_file`.
#[cfg(not(desktop))]
#[tauri::command]
pub fn notes_open_file(vault_id: String, rel_path: String) -> Result<(), IpcError> {
    let _ = (vault_id, rel_path);
    Err(to_ipc_error(CoreError::Unsupported(
        "opening a linked file is desktop-only".to_owned(),
    )))
}

/// The tray's folder-sync snapshot, re-exported so the ~1 Hz tick reaches it
/// the same way it reaches [`recording_snapshot`].
#[cfg(desktop)]
pub fn sync_tray_snapshot(
    app: &tauri::AppHandle,
) -> (keeper_sync::progress::TraySyncState, String) {
    crate::sync_ipc::tray_snapshot(app)
}

/// Which `git` folder sync can drive here (Story 34.14).
///
/// Four failures rather than one, because they need four different next steps:
/// "install git" is useless advice for a git that is installed and too old, and
/// "upgrade git" is useless for one whose system `gitconfig` is malformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum SyncGitState {
    /// This build has no folder sync at all (iOS). The report renders nothing —
    /// telling a phone user about a git version floor would be noise.
    Unsupported,
    /// A binary cleared the floor. `CapabilitiesVm.sync` is `true` exactly here.
    Ok,
    /// Nothing called `git` at any candidate path.
    Missing,
    /// Something is there and cannot run: a stray non-executable file, or a git
    /// that exits non-zero on `--version`.
    Unusable,
    /// It runs and reports a version below the floor.
    TooOld,
}

/// The resolved `git`, or why there isn't one — Settings → Sync's report.
///
/// This exists because the refusal used to be invisible. `Engine::open` rejects a
/// git below the floor, the shell logged that at `debug` (opt-in, so nobody saw
/// it), and the capability probe disagreed with the engine — so the Sync surface
/// could appear and then do nothing at all.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncGitVm {
    pub state: SyncGitState,
    /// `git 2.52 at /opt/homebrew/bin/git (clears the 2.42 floor)` when one was
    /// chosen. Worded as `keeper-syncd doctor` words it — one fact, one spelling.
    pub summary: Option<String>,
    /// Why nothing was chosen, naming every candidate that was tried and what
    /// to do about it. `None` when a binary was chosen.
    pub problem: Option<String>,
    /// The path the owner set explicitly, or `None` for automatic resolution.
    /// Present even when it was rejected: a field that cleared itself on a bad
    /// value would be a silent fallback to auto, which is the defect being fixed.
    pub configured_path: Option<String>,
}

/// Resolve `git` and project the answer, without opening the engine.
///
/// Deliberately engine-free: the whole point is that it answers on exactly the
/// machines where the engine will not open.
fn git_report(state: &AppState) -> SyncGitVm {
    #[cfg(desktop)]
    {
        use keeper_sync::GitReject;

        let platform = state.platform.as_ref();
        let resolution = crate::sync::git_resolution(platform);
        let rejected = resolution.rejected();
        let vm_state = if resolution.chosen().is_some() {
            SyncGitState::Ok
        } else if rejected
            .iter()
            .any(|r| matches!(r.cause, GitReject::TooOld { .. }))
        {
            // Ranked, not first-wins: "you have git, upgrade it" is the most
            // actionable thing to say when a search met several kinds of failure.
            SyncGitState::TooOld
        } else if rejected.iter().any(|r| {
            matches!(
                r.cause,
                GitReject::Unusable { .. } | GitReject::NotExecutable
            )
        }) {
            SyncGitState::Unusable
        } else {
            SyncGitState::Missing
        };
        SyncGitVm {
            state: vm_state,
            summary: resolution.summary(),
            problem: match resolution.chosen() {
                Some(_) => None,
                None => Some(resolution.refusal()),
            },
            configured_path: crate::sync::configured_git_path(platform),
        }
    }
    #[cfg(not(desktop))]
    {
        let _ = state;
        SyncGitVm {
            state: SyncGitState::Unsupported,
            summary: None,
            problem: None,
            configured_path: None,
        }
    }
}

/// The `git` report Settings → Sync renders.
///
/// Registered on every platform (unlike the rest of the sync surface) so the
/// frontend can ask one question and get `unsupported` on a phone instead of an
/// `invoke` rejection it would have to special-case.
#[tauri::command]
pub fn sync_git_status(state: State<'_, AppState>) -> Result<SyncGitVm, IpcError> {
    Ok(git_report(&state))
}

/// Point keeper at a specific `git` binary, or clear the choice with `""`.
///
/// Returns the fresh report rather than `()`, and **does not reject** a path
/// that fails: the path is stored, the refusal is reported, and nothing falls
/// back to a PATH search. Rejecting-without-storing would leave a mistyped path
/// silently reverted to automatic, which is the same class of silent
/// substitution this whole story removes.
///
/// The report is composed *after* [`crate::sync::repoint_engine`] so it
/// describes the process that is now running rather than the one that was: the
/// engine caches a `GitCli` built from the old setting, so a write that only
/// forgot the resolution changed this report and `CapabilitiesVm.sync` while
/// every push, merge and worktree call kept using the previous binary — the
/// capability disagreeing with the engine again, which is the thing this
/// command exists to stop.
#[tauri::command]
pub fn sync_git_path_set(state: State<'_, AppState>, path: String) -> Result<SyncGitVm, IpcError> {
    #[cfg(desktop)]
    {
        let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
        keeper_core::registry::set_sync_git_path(&data_dir, path.trim()).map_err(to_ipc_error)?;
        crate::sync::repoint_engine(Arc::clone(&state.platform));
        Ok(git_report(&state))
    }
    #[cfg(not(desktop))]
    {
        let _ = (state, path);
        Err(IpcError {
            code: IpcErrorCode::Unsupported,
            message: "folder sync is not available on this platform".to_owned(),
            account_id: None,
            retriable: false,
        })
    }
}

/// Where every file-set setting came from, and everything wrong with the
/// settings files (Story 46.7, AD-98).
///
/// **This command is the whole of AD-98's second half.** The layer stack makes
/// a file keep winning; without a surface that says so, the visible effect is a
/// switch that flips back on its own, which is worse than the destructive
/// import it replaced — that one at least only lost the file's value once. So
/// the answer to "where did this value come from?" is a first-class read, not a
/// debug affordance.
///
/// Registered on every platform, like [`sync_git_status`] and unlike the rest
/// of the sync surface: `~/.keeper/keeper.toml` is read by
/// [`keeper_core::config`], which has no desktop gate, and a phone that
/// answered `Command config_layers not found` would force the Settings surface
/// to special-case a question it can always answer honestly (with an empty
/// stack).
///
/// No `State` parameter, deliberately: the stack is process-global and was
/// installed at phase one of `setup()`, before `AppState` had anything in it
/// worth reading. Taking the state would imply this depends on it.
#[tauri::command]
pub fn config_layers() -> Result<ConfigLayersVm, IpcError> {
    let vm = ConfigLayersVm::new(
        keeper_core::config::overrides(),
        keeper_core::config::faults(),
        keeper_core::config::main_folder(),
    );
    // The two fault sources meet here and nowhere earlier. AD-40 keeps
    // `keeper-sync` free of `keeper-core` and `keeper-core` free of
    // `keeper-sync` (`bun run check:core-sync-free` asserts both edges), so the
    // shell is the only crate that can see a folder tier's faults and the app
    // layers' faults at the same time. A user does not care which crate
    // noticed; one list.
    #[cfg(desktop)]
    let vm = vm.with_folder_faults(
        keeper_sync::profile::folder_faults()
            .iter()
            // Fully qualified rather than imported: the only use is inside this
            // `cfg`, and an import would be an unused-import warning on iOS.
            .map(|fault| keeper_core::vm::ConfigFaultVm::folder(&fault.path, fault.message.clone()))
            .collect(),
    );
    Ok(vm)
}

/// Return the data-driven bridge catalog (Story 6.1, FR-42). A one-shot read of
/// the embedded, versioned `risk-tiers.json`, projected into the flat set of
/// surfaced [`BridgeNetworkVm`]s (out-of-scope tier excluded). Carries only static
/// non-secret data — no session, network, or discovery I/O. On a malformed embedded
/// data file the `BridgeError` funnels through [`to_ipc_error`] to `internal`
/// (non-retriable) so the Bridges view can show an error state.
#[tauri::command]
pub fn bridge_catalog() -> Result<Vec<BridgeNetworkVm>, IpcError> {
    keeper_core::bridges::catalog().map_err(|e| to_ipc_error(e.into()))
}

/// Run zero-config, per-Account bridge discovery (Story 6.2, FR-25, AD-16). A
/// one-shot pass that merges three sources — `thirdparty/protocols`, a known-bot
/// MXID probe, and a joined-room `m.bridge` portal / bot-DM scan — into a per-Network
/// [`BridgeStatus`](keeper_core::vm::BridgeStatus), catalog-gated to the surfaced 6.1
/// networks. Resolves with a [`BridgeDiscoveryVm`] (the account's `homeserver` server
/// name + discovered networks; an empty list is the honest "no bridges found" state,
/// not an error). A homeserver lacking `thirdparty/protocols` degrades to the other
/// two sources rather than erroring. A registered-but-not-live account is activated
/// on demand (the First-Run Wizard reaches discovery right after login, before any
/// room-list subscription). Failures funnel through [`to_ipc_error`]: an account id
/// absent from the registry → `internal` (non-retriable), a total transport failure →
/// `syncUnavailable` (retriable). No bot MXID, token, or session material crosses IPC.
#[tauri::command]
pub async fn bridge_discover(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<BridgeDiscoveryVm, IpcError> {
    state
        .accounts
        .discover_bridges(&state.platform, &account_id)
        .await
        .map_err(to_ipc_error)
}

/// Start a native bridge login for `network_id` (Story 6.3, FR-26, AD-16).
///
/// Connects the [`Provisioning`](keeper_core::bridges::transport::provisioning) transport
/// (a data-driven base-URL probe authenticated with the account's Matrix access token as
/// Bearer — the token is read in Rust and never crosses IPC), then streams a
/// [`BridgeLoginVm`] state machine (choosing method → waiting → QR / code entry →
/// success / failure) over `channel` and returns the `session_id` used to submit input /
/// cancel. An unreachable provisioning API or an unknown account funnels through
/// [`to_ipc_error`] (`syncUnavailable` / `internal`). Only rendered VM state crosses IPC.
#[tauri::command]
pub async fn bridge_login_start(
    state: State<'_, AppState>,
    account_id: String,
    network_id: String,
    channel: Channel<BridgeLoginVm>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |vm: BridgeLoginVm| channel.send(vm).is_ok());
    state
        .accounts
        .start_bridge_login(&state.platform, &account_id, &network_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Submit input into a running bridge login (Story 6.3): a flow choice (from the
/// choosing-method phase) or the entered field values (from the code-entry phase). A
/// stale `session_id` funnels through [`to_ipc_error`] (`syncUnavailable`). Entered
/// values ride only inside the [`BridgeLoginInput`] and are never logged.
#[tauri::command]
pub async fn bridge_login_submit(
    state: State<'_, AppState>,
    account_id: String,
    session_id: u64,
    input: BridgeLoginInput,
) -> Result<(), IpcError> {
    state
        .accounts
        .submit_bridge_login(&state.platform, &account_id, session_id, input)
        .await
        .map_err(to_ipc_error)
}

/// Cancel a running bridge login (Story 6.3) — the user closed the Sheet / pressed Esc.
/// Drops the session, best-effort POSTs `/login/cancel/{login_id}` on the retained
/// transport (when the login id has resolved), then aborts the driver task. Idempotent —
/// cancelling an unknown session is a no-op.
#[tauri::command]
pub async fn bridge_login_cancel(
    state: State<'_, AppState>,
    account_id: String,
    session_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .cancel_bridge_login(&account_id, session_id)
        .await;
    Ok(())
}

/// Return the `bbctl` self-host capability for the "Run your own bridge" surface
/// (Story 6.7, FR-29). A one-shot read of the embedded `bbctl.json` (guided-install
/// steps + the supported self-hostable networks) plus the live sidecar availability
/// probe, projected into a [`BbctlAvailabilityVm`]. `available: false` renders the
/// guided-install branch and everything else in keeper keeps working. No token,
/// session, or process material crosses IPC. A malformed embedded data file funnels
/// through [`to_ipc_error`] (`internal`).
#[tauri::command]
pub fn bbctl_availability(state: State<'_, AppState>) -> Result<BbctlAvailabilityVm, IpcError> {
    let runner = DesktopBbctlRunner::new(state.platform.clone());
    state
        .accounts
        .bbctl_availability(&runner)
        .map_err(to_ipc_error)
}

/// Start a `bbctl` self-hosted-bridge run for `network_id` (Story 6.7, FR-29, AD-16).
///
/// Gates the request in the core FIRST (defense in depth): the account must be Beeper
/// (read from the durable, non-secret registry `provider` — never a token) and the
/// network must be self-hostable, else an honest [`BridgeError::Bbctl`] funnels
/// through [`to_ipc_error`] before anything spawns. Then registers the run session in
/// the runs registry **before** spawning the driver task (insert-then-spawn), dedupes
/// an already-in-flight run for the same `(account, network)` (replacing it rather
/// than spawning a second unsupervised daemon), and streams a [`BbctlProgressVm`]
/// stepper (checking → registering → starting → running → success/failure) over
/// `channel`, returning the `sessionId` used to cancel. Only rendered VM state
/// crosses IPC — no token, no raw `bbctl` log line.
#[tauri::command]
pub async fn bbctl_run_start(
    state: State<'_, AppState>,
    account_id: String,
    network_id: String,
    channel: Channel<BbctlProgressVm>,
) -> Result<u64, IpcError> {
    // Gate + resolve the network in the core before any spawn.
    let network = state
        .accounts
        .bbctl_run_start(&state.platform, &account_id, &network_id)
        .map_err(to_ipc_error)?;

    let runner = DesktopBbctlRunner::new(state.platform.clone());
    let sink: keeper_core::bridges::bbctl::BbctlSink =
        Box::new(move |vm: BbctlProgressVm| channel.send(vm).is_ok());

    let registry = state.bbctl_runs.clone();
    let session_id = registry.next_id();

    let bbctl_name = network.bbctl_name.clone();
    let network_owned = network_id.clone();
    let account_owned = account_id.clone();
    let reaper = registry.clone();
    // Reserve the target (aborting any prior in-flight run for it), spawn the driver,
    // and register its abort handle — atomically under one lock, so a racing second
    // start always dedupes and a fast-terminating task cannot leak a resident handle.
    registry.start(&account_id, &network_id, session_id, move || {
        tokio::spawn(async move {
            keeper_core::bridges::bbctl::run_self_hosted(
                &runner,
                &network_owned,
                &bbctl_name,
                sink,
            )
            .await;
            // A naturally-completed run reaps its own registry entry.
            reaper.finish(&account_owned, &network_owned, session_id);
        })
        .abort_handle()
    });

    Ok(session_id)
}

/// Cancel a running `bbctl` self-hosted-bridge run (Story 6.7) — the user closed the
/// run Sheet. Aborts the driver task and removes it from the runs registry.
/// Idempotent — cancelling an unknown session is a no-op. (The launched `bbctl run`
/// daemon is launch-and-leave, so this only tears down keeper's streaming task, not
/// the already-detached bridge process — supervision is out of scope, v1.x.)
#[tauri::command]
pub fn bbctl_run_cancel(state: State<'_, AppState>, session_id: u64) -> Result<(), IpcError> {
    state.bbctl_runs.cancel(session_id);
    Ok(())
}

/// Resolve-or-create the Bridge Bot DM room for `network_id` (Story 6.4, FR-27,
/// UX-DR19) and return its room id, so the frontend can navigate straight to the raw
/// Bridge Bot chat — the manual escape hatch offered from the card Manage menu and a
/// login failure. An unknown account funnels through [`to_ipc_error`] (`internal`); an
/// unresolvable / uncreatable bot DM funnels to `syncUnavailable` (retriable). No bot
/// MXID or session material crosses IPC — only the non-secret room id.
#[tauri::command]
pub async fn bridge_bot_room(
    state: State<'_, AppState>,
    account_id: String,
    network_id: String,
) -> Result<String, IpcError> {
    state
        .accounts
        .bridge_bot_room(&state.platform, &account_id, &network_id)
        .await
        .map_err(to_ipc_error)
}

/// Return the data-driven new-chat resolve capability for `network_id` (Story 6.6,
/// FR-32). A pure, I/O-free projection of the embedded `resolve-support.json`
/// (override-or-default) into a [`ResolveSupportVm`] — the frontend disables the
/// identifier field and shows "not supported on {Network}" upfront when `supported`
/// is `false`, before any resolve call. A malformed embedded data file funnels
/// through [`to_ipc_error`] to `internal`.
#[tauri::command]
pub fn bridge_resolve_support(
    state: State<'_, AppState>,
    network_id: String,
) -> Result<ResolveSupportVm, IpcError> {
    state
        .accounts
        .bridge_resolve_support(&network_id)
        .map_err(to_ipc_error)
}

/// Resolve a new-chat `identifier` on `network_id` through the bridge's provisioning
/// API (Story 6.6, FR-32) and return the portal room id to open. The Rust core
/// connects the provisioning transport (Matrix access token as Bearer, read in Rust
/// and never crossing IPC), calls `resolve_identifier` then `create_dm` only if no DM
/// exists yet, and returns a [`NewChatResolutionVm`] carrying only the non-secret
/// room id (opened verbatim via `roomsStore.selectRoom`). Failures funnel through
/// [`to_ipc_error`]: an unknown account → `internal`; a bot-only account or an
/// unresolvable identifier → `syncUnavailable` (retriable) with the bridge's own
/// verbatim message, so the dialog can render "Not found on {Network}" and retain the
/// input.
#[tauri::command]
pub async fn resolve_bridge_identifier(
    state: State<'_, AppState>,
    account_id: String,
    network_id: String,
    identifier: String,
) -> Result<NewChatResolutionVm, IpcError> {
    state
        .accounts
        .resolve_bridge_identifier(&state.platform, &account_id, &network_id, &identifier)
        .await
        .map_err(to_ipc_error)
}

/// Subscribe to live bridge-session health across every active account (Story 6.5,
/// FR-28, NFR-6, AD-16, UX-DR8/UX-DR11).
///
/// Bootstraps the monitored (logged-in) sessions from each account's discovery pass,
/// spawns a per-account health monitor (management-room notice classifier + a bounded
/// liveness tick), and streams a whole-set [`BridgeHealthSnapshot`] over `channel` —
/// the bootstrap snapshot on subscribe, then only on a per-session state change
/// (diffed). Returns the subscription id; [`bridge_unsubscribe_health`] tears it down.
/// Health is computed entirely in Rust — the frontend mirrors the stream and never
/// re-derives it. No bot MXID, token, or session material crosses IPC — only non-secret
/// render data. Best-effort: a per-account discovery/monitor failure is skipped, so
/// subscription never rejects.
#[tauri::command]
pub async fn bridge_subscribe_health(
    state: State<'_, AppState>,
    channel: Channel<BridgeHealthSnapshot>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |snapshot: BridgeHealthSnapshot| channel.send(snapshot).is_ok());
    // Thread the shared `Platform` port so the health machine's FR-28 leg can post the
    // native bridge-disconnected notification (Story 10.4). The notify config lives on the
    // AccountManager and is bound to the aggregator inside `subscribe_bridge_health`.
    Ok(state
        .accounts
        .subscribe_bridge_health(state.platform.clone(), sink)
        .await)
}

/// Unsubscribe the bridge-health subscription (Story 6.5), draining every per-account
/// monitor (aborting its tick + removing its management-room handlers). Idempotent — a
/// mismatched/unknown id is a no-op.
#[tauri::command]
pub async fn bridge_unsubscribe_health(
    state: State<'_, AppState>,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_bridge_health(subscription_id)
        .await;
    Ok(())
}

/// Open the demo subscription. Emits the snapshot-then-diff batches produced by
/// the tauri-free core over `channel` in order, then returns the subscription
/// id. The first batch delivered is always the snapshot.
#[tauri::command]
pub fn demo_subscribe(channel: Channel<DemoBatch>) -> Result<u64, IpcError> {
    let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
    for batch in snapshot_then_diff() {
        channel.send(batch).map_err(|e| {
            to_ipc_error(CoreError::Internal(format!(
                "failed to send demo batch: {e}"
            )))
        })?;
    }
    Ok(subscription_id)
}

/// Password login command (FR-1, FR-5).
///
/// Delegates the full ordered flow (store-less SSS probe → persistent login →
/// Keychain + registry, with rollback on failure) to `keeper-core`. The
/// `password` argument is transient: it drives the SDK login only and is never
/// returned, stored, or logged. On success resolves to a non-secret
/// [`AccountVm`]; on failure funnels the `CoreError` through [`to_ipc_error`].
#[tauri::command]
pub async fn login_password(
    state: State<'_, AppState>,
    homeserver: String,
    username: String,
    password: String,
) -> Result<AccountVm, IpcError> {
    auth::login_password(state.platform.as_ref(), &homeserver, &username, &password)
        .await
        .map_err(to_ipc_error)
}

/// OIDC (OAuth 2.0 / MSC3861) login command (Story 2.2).
///
/// Runs the shared add-account flow with the OIDC mechanism: the whole browser
/// round-trip (open the system browser, await the `keeper://oauth/callback` deep
/// link, finish the token exchange) happens inside the core `authenticate` step.
/// The pending flow is keyed by its OAuth `state` in the shared registry so the
/// deep-link `on_open_url` handler can route the callback back to it; a
/// concurrent `cancel_oidc` aborts it. On success resolves to a non-secret
/// [`AccountVm`]; on failure (unsupported / timed-out / cancelled / failed /
/// non-SSS) funnels the `CoreError` through [`to_ipc_error`]. No token or
/// authorization `code`/`state` ever crosses back to JavaScript.
#[tauri::command]
pub async fn login_oidc(
    state: State<'_, AppState>,
    homeserver: String,
) -> Result<AccountVm, IpcError> {
    auth::login_oidc(
        state.platform.as_ref(),
        &homeserver,
        state.oauth_flows.clone(),
    )
    .await
    .map_err(to_ipc_error)
}

/// Cancel any in-progress OIDC flow(s) (Story 2.2).
///
/// Aborts every pending flow in the registry (there is at most one add-account
/// flow at a time in the UI); the awaiting `authenticate` resolves as cancelled,
/// `add_account` rolls back, and the UI returns quietly to the form. Idempotent —
/// with no pending flow it is a no-op.
#[tauri::command]
pub fn cancel_oidc(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.oauth_flows.cancel_all();
    Ok(())
}

/// Request a Beeper email login code (Story 2.3, step 1). Delegates to the core,
/// which runs `POST /user/login` → `POST /user/login/email` and stores the
/// intermediate request id (keyed by `email`) in the registry so it never
/// crosses IPC. Resolves on success (a code has been emailed); any Beeper failure
/// funnels through [`to_ipc_error`] to the retriable `beeperUnavailable` code. No
/// bearer token, request id, or JWT ever crosses back to JavaScript.
#[tauri::command]
pub async fn beeper_request_code(
    state: State<'_, AppState>,
    email: String,
) -> Result<(), IpcError> {
    state
        .beeper_flows
        .request_code(&email)
        .await
        .map_err(to_ipc_error)
}

/// Complete a Beeper email-code login (Story 2.3, step 2). Delegates to the core,
/// which takes the stored request id for `email`, runs `POST
/// /user/login/response` to obtain the JWT, then completes login via
/// `org.matrix.login.jwt` through the shared add-account pipeline (store-less SSS
/// gate → persistent store → Keychain → registry, with rollback on failure). On
/// success resolves to a non-secret [`AccountVm`]; any Beeper failure (including
/// an abandoned flow with no stored request id) funnels through [`to_ipc_error`]
/// to the retriable `beeperUnavailable` code. The emailed `code` is transient —
/// never returned, stored, or logged.
#[tauri::command]
pub async fn login_beeper(
    state: State<'_, AppState>,
    email: String,
    code: String,
) -> Result<AccountVm, IpcError> {
    state
        .beeper_flows
        .login(state.platform.as_ref(), &email, &code)
        .await
        .map_err(to_ipc_error)
}

/// Cancel the in-progress Beeper login flow for `email` (Story 2.3). Drops that
/// flow's pending request id so no residue lingers; other in-flight Beeper
/// logins are untouched and nothing is persisted. Idempotent — with no pending
/// flow for `email` it is a no-op.
#[tauri::command]
pub fn cancel_beeper(state: State<'_, AppState>, email: String) -> Result<(), IpcError> {
    state.beeper_flows.cancel(&email);
    Ok(())
}

/// Persist the app-wide at-rest encryption posture (Story 2.6, AD-22). Writes
/// `on`/`off` to the `settings` table in `keeper.db`. Sync — the value is a
/// non-secret app-wide flag; the per-account passphrase is generated and stored
/// (Keychain only) later, inside `add_account`. Failures funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub fn set_encryption_posture(state: State<'_, AppState>, enabled: bool) -> Result<(), IpcError> {
    auth::set_encryption_posture(state.platform.as_ref(), enabled).map_err(to_ipc_error)
}

/// Read the app-wide at-rest encryption posture (Story 2.6). Resolves to
/// `Some(true)` (on), `Some(false)` (off), or `None` (unchosen — the fresh-install
/// state that gates the first-run choice). `Option<bool>` serializes to
/// `boolean | null` across IPC. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn encryption_posture(state: State<'_, AppState>) -> Result<Option<bool>, IpcError> {
    auth::get_encryption_posture(state.platform.as_ref()).map_err(to_ipc_error)
}

/// Read the archive-fed edit history for a message from the Local Archive (Story
/// 5.2, FR-11). `itemKey` is the message's opaque render `key` (its `unique_id`);
/// the Rust core resolves it to the *original* event id via the live timeline and
/// reads the version chain from `archive.db` — never a homeserver fetch. Resolves
/// with an ordered `Vec<EditVersionVm>` (oldest→newest, the last flagged
/// `isCurrent`), or an empty array when the item is unresolvable or has no local
/// history. No event id ever crosses IPC. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn edit_history_get(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    item_key: String,
) -> Result<Vec<EditVersionVm>, IpcError> {
    state
        .accounts
        .edit_history(&state.platform, &account_id, &room_id, &item_key)
        .await
        .map_err(to_ipc_error)
}

/// Read the app-wide "honor remote deletions locally" policy (Story 5.2, FR-36).
/// Resolves with `true` only when the setting is explicitly on; absent/off ⇒
/// `false` (preserve). Read-time policy only — flipping it is never retroactive.
/// Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn honor_remote_deletions(state: State<'_, AppState>) -> Result<bool, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::archive::get_honor_remote_deletions(&data_dir).map_err(to_ipc_error)
}

/// Persist the app-wide "honor remote deletions locally" policy (Story 5.2).
/// Writes `on`/`off` to the `settings` table in `keeper.db`. Affects subsequent
/// reads only (not retroactive). Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn set_honor_remote_deletions(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::archive::set_honor_remote_deletions(&data_dir, enabled).map_err(to_ipc_error)
}

/// Persist the composer draft for `(account_id, room_id)` (Story 7.1, AD-15). Upserts
/// `body` verbatim into the `drafts` table in `keeper.db` with the current wall clock
/// as `updated_ts`. The frontend trims before calling and deletes (not saves) an empty
/// body, so a stored row is always non-empty. Sync — a small keeper-local write, never
/// a secret. Failures funnel
/// through [`to_ipc_error`]; the frontend fires this fire-and-forget so a failure never
/// blocks typing. The body is never logged.
#[tauri::command]
pub fn set_draft(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    body: String,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::set_draft(&data_dir, &account_id, &room_id, &body, now_ms())
        .map_err(to_ipc_error)
}

/// Read the composer draft for `(account_id, room_id)` (Story 7.1). Resolves with the
/// stored body or `None` when no draft exists; `Option<String>` serializes to
/// `string | null`. The composer seeds its local state from this on mount. Failures
/// funnel through [`to_ipc_error`].
#[tauri::command]
pub fn get_draft(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<Option<String>, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::get_draft(&data_dir, &account_id, &room_id).map_err(to_ipc_error)
}

/// Delete the composer draft for `(account_id, room_id)` (Story 7.1). Idempotent —
/// clearing an absent draft (send succeeded, or the body trimmed to empty) is a no-op.
/// Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn delete_draft(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::delete_draft(&data_dir, &account_id, &room_id).map_err(to_ipc_error)
}

/// List every draft's `(account_id, room_id)` key (Story 7.1). Presence only — the
/// body is not returned. Seeds the inbox draft markers at startup, cross-account.
/// Serializes to `[accountId, roomId][]`. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn list_drafts(state: State<'_, AppState>) -> Result<Vec<(String, String)>, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::list_drafts(&data_dir).map_err(to_ipc_error)
}

/// Mirror the composer draft for `(account_id, room_id)` to the account (Story 7.2,
/// AD-15): the synced `dev.keeper.draft` account-data event plus a best-effort
/// `save_composer_draft` (Element interop). Async — resolves the live `Room` via
/// `state.accounts`. Deduped by last-mirrored body; the `updated_ts` is generated in
/// Rust at write time (a stale caller timestamp is never trusted).
///
/// Best-effort: the frontend fires this off the debounced keystroke path and swallows
/// any rejection — a mirror failure must never block or fail local persistence, so the
/// only symptom is the absent cross-device echo. The body is never logged.
#[tauri::command]
pub async fn mirror_draft(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    body: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .mirror_draft(&account_id, &room_id, &body)
        .await
        .map_err(to_ipc_error)
}

/// Clear the draft mirror for `(account_id, room_id)` (Story 7.2): tombstone the
/// `dev.keeper.draft` account-data event plus `clear_composer_draft`, so other devices
/// stop showing the draft. Async — resolves via `state.accounts`. Best-effort: fired
/// fire-and-forget on the clear path; a failure never blocks the send/clear and can at
/// worst transiently re-present a cleared draft cross-device (never destroys text).
#[tauri::command]
pub async fn clear_draft_mirror(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .clear_draft_mirror(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Read the remote (cross-device) draft for `(account_id, room_id)` from the
/// account-data mirror (Story 7.2), or `None` when there is no draft (an empty-body
/// tombstone maps to `None`). Async — resolves via `state.accounts`. Read only to
/// *offer* adoption — local always wins; the composer never auto-replaces non-empty
/// local text. A failure funnels through [`to_ipc_error`]; the composer falls back to
/// local. The body is never logged.
#[tauri::command]
pub async fn load_remote_draft(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<Option<RemoteDraftVm>, IpcError> {
    state
        .accounts
        .load_remote_draft(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// List every pending draft across all accounts for the approval pane (Story 7.3).
/// Async — reads the full draft rows from `keeper.db` and enriches each with the
/// owning account's identity/hue (registry) plus the room's display name + bridge
/// network (best-effort via the live `Room`). A draft whose room/account cannot be
/// resolved is STILL listed (`display_name = room_id`, `network = None`) — the
/// airlock never hides held text. Bodies stay authoritative in Rust; never logged.
/// Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn list_pending_drafts(
    state: State<'_, AppState>,
) -> Result<Vec<ApprovalDraftVm>, IpcError> {
    state
        .accounts
        .list_pending_drafts(&state.platform)
        .await
        .map_err(to_ipc_error)
}

/// Approve (send) a pending draft's `body` to `(account_id, room_id)` through the
/// single dispatch gate with the `ApprovalPaneApprove` trigger (FR-41, AD-13, Story
/// 7.3). Async — delegates to the core, which enqueues the message on the room's
/// open `Timeline`; the local echo and every send-state transition arrive back over
/// the existing timeline subscription (no echo is synthesized). An enqueue-time
/// failure funnels through [`to_ipc_error`] to `SendFailed`; the frontend retains
/// the draft on error so a failed send never loses unsent text.
#[tauri::command]
pub async fn approve_draft(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    body: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .send_approval(&state.platform, &account_id, &room_id, &body)
        .await
        .map_err(to_ipc_error)
}

/// Search the Local Archive with full-text search (Story 5.3, FR-34, AD-12).
///
/// Opens a fresh read-only `archive.db` connection (WAL permits concurrent readers,
/// so search never touches the writer or a live Matrix session — it works fully
/// offline), reads the app-wide honor-remote-deletions setting, and runs the
/// tauri-free [`keeper_core::archive::search`] engine: trigram MATCH for queries of
/// ≥3 Unicode scalar values, an accelerated `LIKE` scan below that, applying the
/// account / room / sender / date-range filters, honoring deletions when enabled,
/// and deduplicating to one [`SearchHitVm`] per logical message (chain-root
/// `eventId`). Resolves with the hits (an empty array on no match — never an
/// error). Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn search_archive(
    state: State<'_, AppState>,
    filter: SearchFilterVm,
) -> Result<Vec<SearchHitVm>, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    // A fresh install (or an account that has never synced) has no `archive.db` yet;
    // an empty archive means empty results, not an error dialog. Opening a missing
    // file read-only would otherwise fail with `SQLITE_CANTOPEN`.
    if !keeper_core::archive::db::db_path(&data_dir).exists() {
        return Ok(Vec::new());
    }
    let honor_deletions =
        keeper_core::archive::get_honor_remote_deletions(&data_dir).map_err(to_ipc_error)?;
    // A fresh read-only connection: WAL readers never block the single writer, and
    // search must not require a live session (works offline / after sign-out).
    let conn = keeper_core::archive::db::open_readonly_archive_db(&data_dir)
        .map_err(CoreError::from)
        .map_err(to_ipc_error)?;
    let domain_filter = keeper_core::archive::SearchFilter::from(filter);
    keeper_core::archive::search(&conn, &domain_filter, honor_deletions)
        .map_err(CoreError::from)
        .map_err(to_ipc_error)
}

/// Search the recordings archive for the Recordings browser (Story 42.3,
/// FR-141, UX-DR50).
///
/// [`search_archive`]'s shape, deliberately: this is the same question about a
/// different table, and a user who has learned how search behaves in one
/// surface has learned it in both. So — a fresh READ-ONLY `archive.db`
/// connection per query (WAL admits concurrent readers, so browsing never
/// touches the recorder's writer and works with no session and no network), the
/// tauri-free Story 42.2 engine behind a `Vm` seam, and failures through the
/// one [`to_ipc_error`] funnel.
///
/// **An absent `archive.db` is an empty result, not an error dialog.** A
/// machine that has never recorded anything has no archive to open, and
/// `SQLITE_OPEN_READ_ONLY` on a missing file is `SQLITE_CANTOPEN` — a
/// first-run failure the user can neither understand nor act on. The frontend
/// tells "nothing recorded yet" from "nothing matches this filter" from the
/// filter it sent, which is the only place both facts are known.
///
/// The effective recordings destination (Story 41.2) is resolved here and each
/// row's absolute path composed from it, so no frontend surface ever joins a
/// root to a subfolder (AD-65) and Reveal cannot open a folder the recorder
/// would not have written to.
///
/// Returns the page AND the archive-wide count (Story 44.11): the page has
/// always stopped at `recordings_fts::DEFAULT_LIMIT`, and a surface that
/// counted its own array would say "200 sessions" to somebody with nine
/// thousand.
#[tauri::command]
pub fn search_recordings(
    state: State<'_, AppState>,
    filter: RecordingFilterVm,
) -> Result<RecordingSearchVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let destination_root = effective_destination_dir(&data_dir, &state.platform);
    search_recordings_in(&data_dir, &destination_root, filter)
}

/// The whole of [`search_recordings`] except the two answers only the app state
/// can give, so every rule above is asserted over a temp directory with no
/// Tauri app, no registry and no `git` — including the first-run one, which is
/// exactly the machine that has neither.
fn search_recordings_in(
    data_dir: &Path,
    destination_root: &Path,
    filter: RecordingFilterVm,
) -> Result<RecordingSearchVm, IpcError> {
    if !keeper_core::archive::db::db_path(data_dir).exists() {
        // No archive is zero sessions, and zero is a number the surface prints.
        return Ok(RecordingSearchVm {
            rows: Vec::new(),
            total: 0,
        });
    }
    let conn = keeper_core::archive::db::open_readonly_archive_db(data_dir)
        .map_err(CoreError::from)
        .map_err(to_ipc_error)?;
    let domain_filter = keeper_core::archive::RecordingFilter::from(filter);
    keeper_core::archive::recordings_fts::search_recording_vms(
        &conn,
        &domain_filter,
        destination_root,
    )
    .map_err(CoreError::from)
    .map_err(to_ipc_error)
}

/// Everything a recording note's reader can act on, for the session the note
/// names by id (Story 42.4, FR-142, FR-145, AD-65).
///
/// A recording note carries only relative paths — `recording:` and the entries
/// of `files:` — because FR-145 keeps absolute paths out of a file the user
/// syncs between machines. That leaves the reader holding text that names a
/// recording and cannot open it, and AD-65 forbids the surface from making it
/// openable by joining a root onto it. This command is that join, composed
/// here from the EFFECTIVE recordings destination (Story 41.2) exactly as
/// [`search_recordings`] composes a row's, so the two surfaces can never
/// disagree about where a session is.
///
/// **By session id, because that is the handle that survives.** Story 40.4
/// renames a session's folder and Story 42.1's row follows it, so the index
/// knows where the recording is NOW while a note written before the rename
/// still says where it was. The note keeps its own text; the actions follow
/// the index.
///
/// `None` — never an error — for a session no archive row knows, for one whose
/// folder is not on this machine, and on a first run with no `archive.db` at
/// all. All three mean the same thing to the person reading the note, and the
/// surface answers all three by rendering the note's text with no action
/// attached: an affordance that opens nothing is worse than an absent one.
#[tauri::command]
pub fn recording_note_targets(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<Vec<RecordingNoteTargetVm>>, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let destination_root = effective_destination_dir(&data_dir, &state.platform);
    recording_note_targets_in(&data_dir, &destination_root, &session_id)
}

/// The whole of [`recording_note_targets`] except the two answers only the app
/// state can give — [`search_recordings_in`]'s split, and for its reason: the
/// rules above are then asserted over a temp directory with no Tauri app and
/// no registry, including the first-run one.
///
/// `pub(crate)` because the `keeper-recording://` handler resolves through this
/// too (Story 42.4): a protocol handler that answered from its own lookup would
/// be a second opinion on where a session's files are, and the note's actions
/// and its embedded player would drift apart after a Story 40.4 retitle.
pub(crate) fn recording_note_targets_in(
    data_dir: &Path,
    destination_root: &Path,
    session_id: &str,
) -> Result<Option<Vec<RecordingNoteTargetVm>>, IpcError> {
    if !keeper_core::archive::db::db_path(data_dir).exists() {
        return Ok(None);
    }
    let conn = keeper_core::archive::db::open_readonly_archive_db(data_dir)
        .map_err(CoreError::from)
        .map_err(to_ipc_error)?;
    keeper_core::archive::recordings_fts::session_note_targets(&conn, session_id, destination_root)
        .map_err(CoreError::from)
        .map_err(to_ipc_error)
}

/// Start a background archive export (Story 5.5, FR-35, AD-11).
///
/// Registers a cancel flag, returns the `exportId` immediately, and spawns a
/// blocking job (rusqlite is synchronous) that reads `archive.db` **only** via a
/// fresh read-only connection — never the SDK store, live session, or network, so a
/// signed-out Account is still exportable. The job streams [`ExportProgressVm`]
/// batches over `channel` (`Running` heartbeats, then exactly one terminal
/// `Completed`/`Cancelled`/`Failed`), best-effort-copies media via the injected
/// resolver (currently `None` — session-free media byte inclusion is deferred, so
/// every media item is skipped-and-counted, honoring AD-11), and on cancel/failure
/// deletes the partial scope folder before the terminal batch. The job deregisters
/// its cancel flag on any terminal phase. Setup failures (data dir / missing
/// archive) funnel through [`to_ipc_error`].
#[tauri::command]
pub fn export_start(
    state: State<'_, AppState>,
    request: ExportRequestVm,
    channel: Channel<ExportProgressVm>,
) -> Result<u64, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    // Read the honor-remote-deletions policy once (the same accessor search uses),
    // so a redacted root renders a stub and never the withheld content.
    let honor_deletions =
        keeper_core::archive::get_honor_remote_deletions(&data_dir).map_err(to_ipc_error)?;

    let (export_id, cancel) = state.exports.register();
    let exports = state.exports.clone();

    // The blocking job owns its own read-only connection and runs off the async
    // runtime so it never blocks messaging (AD-11). A closed channel simply drops
    // the batch (the frontend unsubscribed).
    tokio::task::spawn_blocking(move || {
        run_export_job(
            &data_dir,
            &request,
            honor_deletions,
            &cancel,
            export_id,
            &channel,
        );
        // Terminal phase reached (or the job never started): deregister the flag.
        exports.deregister(export_id);
    });

    Ok(export_id)
}

/// The blocking export body (Story 5.5). Opens a read-only `archive.db`, runs the
/// tauri-free [`keeper_core::archive::export::run_export`], and sends the terminal
/// batch. All errors are converted into a terminal `Failed`/`Cancelled` batch — the
/// caller (`export_start`) already returned the `exportId`, so nothing rejects here.
fn run_export_job(
    data_dir: &std::path::Path,
    request: &ExportRequestVm,
    honor_deletions: bool,
    cancel: &AtomicBool,
    export_id: u64,
    channel: &Channel<ExportProgressVm>,
) {
    use keeper_core::archive::export::{run_export, ExportError};

    // A fresh install / never-synced account has no `archive.db`; treat it as an
    // empty archive that exports cleanly rather than an error.
    let dest_root = std::path::PathBuf::from(&request.destination_dir);
    let conn = if keeper_core::archive::db::db_path(data_dir).exists() {
        match keeper_core::archive::db::open_readonly_archive_db(data_dir) {
            Ok(conn) => Some(conn),
            Err(e) => {
                send_terminal_failed(channel, export_id, e.to_string());
                return;
            }
        }
    } else {
        None
    };

    // The progress sink: forward each `Running` batch to the channel (a closed
    // channel drops it — the frontend unsubscribed).
    let progress = |vm: ExportProgressVm| channel.send(vm).is_ok();

    // The media resolver is injected here to keep `keeper-core` session-free. Full
    // session-free media byte inclusion is out of scope for Story 5.5 (deferred), so
    // pass `None`: every media item is skipped-and-counted, honoring AD-11.
    let media_resolver = None;

    let result = match &conn {
        Some(conn) => run_export(
            conn,
            request,
            &dest_root,
            honor_deletions,
            &progress,
            cancel,
            media_resolver,
            export_id,
        ),
        None => {
            // No archive on disk: run against a throwaway in-memory DB with the
            // `events` schema so the export produces valid empty output.
            match keeper_core::archive::db::open_empty_in_memory_archive_db() {
                Ok(conn) => run_export(
                    &conn,
                    request,
                    &dest_root,
                    honor_deletions,
                    &progress,
                    cancel,
                    media_resolver,
                    export_id,
                ),
                Err(e) => {
                    send_terminal_failed(channel, export_id, e.to_string());
                    return;
                }
            }
        }
    };

    match result {
        Ok(outcome) => {
            let _ = channel.send(ExportProgressVm {
                export_id,
                phase: ExportPhase::Completed,
                messages_written: outcome.messages_written,
                total_messages: Some(outcome.messages_written),
                media_copied: outcome.media_copied,
                media_skipped: outcome.media_skipped,
                output_paths: outcome.output_paths,
                error: None,
            });
        }
        Err(ExportError::Cancelled) => {
            let _ = channel.send(ExportProgressVm {
                export_id,
                phase: ExportPhase::Cancelled,
                messages_written: 0,
                total_messages: None,
                media_copied: 0,
                media_skipped: 0,
                output_paths: Vec::new(),
                error: None,
            });
        }
        Err(ExportError::Failed(e)) => {
            send_terminal_failed(channel, export_id, e.to_string());
        }
    }
}

/// Send a terminal `Failed` export batch (Story 5.5). The message is a non-secret
/// description — never message content or media bytes.
fn send_terminal_failed(channel: &Channel<ExportProgressVm>, export_id: u64, message: String) {
    let _ = channel.send(ExportProgressVm {
        export_id,
        phase: ExportPhase::Failed,
        messages_written: 0,
        total_messages: None,
        media_copied: 0,
        media_skipped: 0,
        output_paths: Vec::new(),
        error: Some(message),
    });
}

/// Cancel a running archive export by id (Story 5.5). Sets the job's shared cancel
/// flag; the synchronous export loop stops at its next between-events check, deletes
/// partial output, and streams the `Cancelled` terminal batch. Idempotent — a no-op
/// for an unknown / already-finished id.
#[tauri::command]
pub fn export_cancel(state: State<'_, AppState>, export_id: u64) -> Result<(), IpcError> {
    state.exports.cancel(export_id);
    Ok(())
}

/// Reveal an exported file in the OS file manager (Story 5.5, "Reveal in Finder").
/// Delegates to `tauri_plugin_opener::reveal_item_in_dir` (the `opener:default`
/// capability grants `allow-reveal-item-in-dir`). An invalid / non-existent path
/// maps to a non-retriable internal `IpcError` — never a panic.
#[cfg(desktop)]
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), IpcError> {
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| {
        to_ipc_error(CoreError::Internal(format!(
            "could not reveal the file: {e}"
        )))
    })
}

/// Mobile stub for [`reveal_path`] (Story 12.2): there is no user-visible file
/// manager to reveal into on iOS — an honest `Unsupported` (`retriable: false`)
/// through the single `to_ipc_error` funnel. The `revealInFileManager` capability
/// is reported `false`, so Epic 13 hides the affordance before it is ever invoked.
#[cfg(not(desktop))]
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), IpcError> {
    let _ = path;
    Err(to_ipc_error(CoreError::Unsupported(
        "revealing a file in the OS file manager is desktop-only".to_owned(),
    )))
}

/// Hand a recording's file to the system's default handler — the Recordings
/// browser's Play (Story 42.3, FR-141, UX-DR50).
///
/// **The containment check comes first, and it is the whole point of this
/// command existing rather than a bare `open_path` binding.** A command that
/// opens whatever path the webview names is a file-disclosure primitive: an
/// injected string is enough to launch the user's default application on any
/// file the process can read. `notes_open_file` refuses for exactly that reason
/// and re-derives its path before `opener` sees it; here the rule is "inside the
/// recordings destination root, and nowhere else", which is where every path
/// this surface can legitimately offer comes from.
///
/// Both halves of the codebase's containment idiom (AD-59), because either
/// alone is defeatable. [`session_relative_key`] is the lexical half — under
/// the root, and every component a plain name, so `..` cannot walk out — and
/// the canonicalizing half catches what no string test can: a symlink planted
/// inside the recordings folder that points somewhere else. Canonicalizing the
/// root too is what keeps a destination that is itself reached through a
/// symlink (`~/Movies` on a machine with a relocated home) from refusing every
/// file under it.
///
/// A path that no longer resolves is refused here rather than at the opener,
/// which is the honest answer for a session whose folder was moved or deleted
/// outside keeper.
#[cfg(desktop)]
#[tauri::command]
pub fn recording_open_path(state: State<'_, AppState>, path: String) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let root = effective_destination_dir(&data_dir, &state.platform);
    let target = contained_recording_path(&root, &path)?;
    tauri_plugin_opener::open_path(&target, None::<&str>).map_err(|error| {
        to_ipc_error(CoreError::Internal(format!(
            "could not open \"{path}\": {error}"
        )))
    })
}

/// The containment check [`recording_open_path`] refuses on, as a function of
/// nothing but a root and a string — so the refusal is asserted over a temp
/// directory, which is the only way a security rule is ever actually tested.
/// Returns the resolved path to open.
#[cfg(desktop)]
fn contained_recording_path(root: &Path, path: &str) -> Result<PathBuf, IpcError> {
    let refuse = |reason: &str| {
        Err(IpcError {
            code: IpcErrorCode::Internal,
            message: format!("\"{path}\" {reason}"),
            account_id: None,
            retriable: false,
        })
    };
    let target = PathBuf::from(path);
    if session_relative_key(root, &target).is_none() {
        return refuse("is not inside the recordings destination, so it will not be opened");
    }
    let (Ok(canonical_root), Ok(canonical_target)) = (root.canonicalize(), target.canonicalize())
    else {
        return refuse("could not be resolved on disk, so there is nothing to open");
    };
    if !canonical_target.starts_with(&canonical_root) {
        return refuse("resolves outside the recordings destination, so it will not be opened");
    }
    Ok(canonical_target)
}

/// Mobile stub for [`recording_open_path`] (Story 42.3): recording — and so the
/// browser over it — is a desktop-only surface, and there is no system handler
/// to hand a file to on iOS. An honest `Unsupported` (`retriable: false`)
/// through the single [`to_ipc_error`] funnel; the `recording` capability is
/// reported `false`, so the surface is absent before this can be invoked.
#[cfg(not(desktop))]
#[tauri::command]
pub fn recording_open_path(state: State<'_, AppState>, path: String) -> Result<(), IpcError> {
    let _ = (state, path);
    Err(to_ipc_error(CoreError::Unsupported(
        "opening a recording is desktop-only".to_owned(),
    )))
}

/// Subscribe to an account's sliding-sync room list (FR-8, AD-8/9/19/20).
///
/// Lazily activates the account (session restore + `SyncService`), then streams
/// [`RoomListBatch`]es over `channel` — a `Reset` snapshot first, then diffs —
/// and returns the subscription id. The sink forwards each batch to the channel;
/// a closed channel simply drops the batch (the frontend has unsubscribed).
#[tauri::command]
pub async fn room_list_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<RoomListBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: RoomListBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_room_list(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one room-list subscription, aborting its producer task
/// (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn room_list_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_room_list(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Subscribe to a room's timeline (FR-8, FR-9, AD-4/AD-8/AD-19).
///
/// Reuses the account's live session (activating it lazily), opens the room's
/// SDK `Timeline`, and streams [`TimelineBatch`]es over `channel` — a `Reset`
/// snapshot first, then diffs — returning the subscription id. The sink forwards
/// each batch to the channel; a closed channel simply drops the batch (the
/// frontend has unsubscribed). A room-not-found / timeline-build failure funnels
/// through [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn timeline_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    channel: Channel<TimelineBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: TimelineBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_timeline(&state.platform, &account_id, &room_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one timeline subscription, aborting its producer task and
/// dropping its `Timeline` (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn timeline_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_timeline(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Send a plain-text message to a room through the single dispatch gate (FR-9,
/// FR-41, AD-13). Delegates to the core, which enqueues the message on the room's
/// open `Timeline`; the local echo and every send-state transition arrive back
/// over the existing timeline subscription (no echo is synthesized). An
/// enqueue-time failure funnels through [`to_ipc_error`] to `SendFailed`.
#[tauri::command]
pub async fn send_text(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    body: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .send_text(&state.platform, &account_id, &room_id, &body)
        .await
        .map_err(to_ipc_error)
}

/// Read the Undo-Send window in whole seconds (Story 8.3). Absent / unparsable ⇒ the
/// default of 10; a stored value is clamped to `0..=60`. Sync — a small keeper-local
/// read. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn undo_send_window(state: State<'_, AppState>) -> Result<u16, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::get_undo_send_window(&data_dir).map_err(to_ipc_error)
}

/// Set the Undo-Send window in whole seconds (Story 8.3), clamped to `0..=60` before
/// persisting (0 disables holding). Sync — a small keeper-local write. Failures funnel
/// through [`to_ipc_error`].
#[tauri::command]
pub fn set_undo_send_window(state: State<'_, AppState>, seconds: u16) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::set_undo_send_window(&data_dir, seconds).map_err(to_ipc_error)
}

/// Build the [`HotkeyVm`] for `accelerator`: `isDefault` vs the shipped default, `active`
/// = whether it is currently registered with the OS, and the soft `conflict` warning.
/// Pure over the app's global-shortcut state and the accelerator string.
#[cfg(desktop)]
fn hotkey_vm(app: &tauri::AppHandle, accelerator: String) -> HotkeyVm {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let is_default = accelerator == crate::hotkey::DEFAULT_HOTKEY;
    // `active` is honest: the parsed accelerator must both parse AND be registered.
    let active = crate::hotkey::parse(&accelerator)
        .map(|shortcut| app.global_shortcut().is_registered(shortcut))
        .unwrap_or(false);
    let conflict = crate::hotkey::known_conflict(&accelerator);
    HotkeyVm {
        accelerator,
        is_default,
        active,
        conflict,
    }
}

/// Read the OS-global summon hotkey binding (Story 9.4, FR-50). Returns the persisted
/// accelerator (absent ⇒ the default `⌃⌥Space`), whether it equals the default, whether
/// it is currently registered with the OS (`active`), and any soft conflict warning.
/// Failures funnel through [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn hotkey_get(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<HotkeyVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let accelerator = keeper_core::registry::get_global_hotkey(&data_dir).map_err(to_ipc_error)?;
    Ok(hotkey_vm(&app, accelerator))
}

/// Mobile stub for [`hotkey_get`] (Story 12.2): there is no OS-global hotkey on
/// iOS — an honest `Unsupported` (`retriable: false`) through `to_ipc_error`. The
/// `globalHotkey` capability is reported `false`, so Epic 13 hides the section.
#[cfg(not(desktop))]
#[tauri::command]
pub fn hotkey_get() -> Result<HotkeyVm, IpcError> {
    Err(to_ipc_error(CoreError::Unsupported(
        "the OS-global summon hotkey is desktop-only".to_owned(),
    )))
}

/// Reassign the OS-global summon hotkey (Story 9.4, FR-50). Validates the accelerator,
/// computes the soft `known_conflict` warning, then unregisters the old binding and
/// registers the new one with the OS; on success persists it and returns the new VM. A
/// malformed accelerator is rejected before touching registration; if the OS *refuses*
/// the new registration — or the OS accepts it but persisting the value fails — the old
/// binding is restored (re-registered) and nothing is persisted, and the command returns
/// `Err`. Logs carry accelerator strings only.
#[cfg(desktop)]
#[tauri::command]
pub fn hotkey_set(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    accelerator: String,
) -> Result<HotkeyVm, IpcError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;

    // Validate before touching registration (malformed → reject, matrix row 8).
    let Some(new_shortcut) = crate::hotkey::parse(&accelerator) else {
        return Err(to_ipc_error(CoreError::Internal(format!(
            "invalid accelerator: {accelerator}"
        ))));
    };

    let previous = keeper_core::registry::get_global_hotkey(&data_dir).map_err(to_ipc_error)?;
    let gs = app.global_shortcut();

    // Unregister the currently-bound accelerator (best-effort — it may already be gone
    // if startup registration failed). Only the single summon hotkey is ever bound.
    if let Some(prev_shortcut) = crate::hotkey::parse(&previous) {
        if gs.is_registered(prev_shortcut) {
            if let Err(error) = gs.unregister(prev_shortcut) {
                tracing::warn!(%error, accelerator = %previous, "hotkey: could not unregister old binding");
            }
        }
    }

    // Register the new accelerator with the shared toggle handler. A hard failure keeps
    // the OLD binding (re-register it) and returns Err — nothing is persisted.
    if let Err(error) = gs.on_shortcut(new_shortcut, crate::hotkey::on_shortcut_event) {
        tracing::warn!(%error, accelerator, "hotkey: OS refused to register new binding; restoring previous");
        // Restore the previous binding so the user is not left with no hotkey. If the
        // restore ALSO fails (e.g. the previous accelerator was never registered), log
        // it — the user is then left with no active hotkey, which `hotkey_get().active`
        // will report as `false` so the Settings section shows the permission
        // explanation rather than failing silently.
        if let Some(prev_shortcut) = crate::hotkey::parse(&previous) {
            if let Err(restore_error) =
                gs.on_shortcut(prev_shortcut, crate::hotkey::on_shortcut_event)
            {
                tracing::warn!(%restore_error, accelerator = %previous, "hotkey: could not restore previous binding after a failed reassignment");
            }
        }
        return Err(to_ipc_error(CoreError::Internal(format!(
            "the system refused to register {accelerator}: {error}"
        ))));
    }

    // Only persist an accelerator the OS accepted (Block-If / never-persist-refused). If
    // the OS accepted the new binding but the persist fails (e.g. a disk error), roll the
    // registration back to `previous` so the live global shortcut and the stored value
    // never diverge — otherwise the new hotkey would be live this session while startup
    // and `hotkey_get` would report the old one, leaving `active=false` for a working key.
    if let Err(error) = keeper_core::registry::set_global_hotkey(&data_dir, &accelerator) {
        tracing::warn!(%error, accelerator, "hotkey: could not persist new binding; rolling back to previous");
        if gs.is_registered(new_shortcut) {
            if let Err(unreg_error) = gs.unregister(new_shortcut) {
                tracing::warn!(%unreg_error, accelerator, "hotkey: could not unregister new binding during rollback");
            }
        }
        if let Some(prev_shortcut) = crate::hotkey::parse(&previous) {
            if let Err(restore_error) =
                gs.on_shortcut(prev_shortcut, crate::hotkey::on_shortcut_event)
            {
                tracing::warn!(%restore_error, accelerator = %previous, "hotkey: could not restore previous binding after a failed persist");
            }
        }
        return Err(to_ipc_error(error));
    }
    Ok(hotkey_vm(&app, accelerator))
}

/// Mobile stub for [`hotkey_set`] (Story 12.2): there is no OS-global hotkey on
/// iOS — an honest `Unsupported` (`retriable: false`) through `to_ipc_error`.
/// Nothing is validated, registered, or persisted.
#[cfg(not(desktop))]
#[tauri::command]
pub fn hotkey_set(accelerator: String) -> Result<HotkeyVm, IpcError> {
    let _ = accelerator;
    Err(to_ipc_error(CoreError::Unsupported(
        "the OS-global summon hotkey is desktop-only".to_owned(),
    )))
}

/// The soft conflict warning shown against the recording accelerator (Story
/// 20.4): the curated macOS system-shortcut list (`known_conflict`) plus one
/// cross-check — a non-empty recording chord equal to the summon binding warns
/// (the OS refusing the duplicate registration is the hard signal; this is the
/// honest soft one). Pure over the two accelerator strings.
#[cfg(desktop)]
fn recording_hotkey_conflict(accelerator: &str, summon: &str) -> Option<String> {
    crate::hotkey::known_conflict(accelerator).or_else(|| {
        // Case-insensitive like `known_conflict`: `control+alt+space` and
        // `Control+Alt+Space` are the same binding to the OS, so the soft clash
        // warning must fire regardless of how the two accelerators are spelled.
        (!accelerator.is_empty() && accelerator.eq_ignore_ascii_case(summon))
            .then(|| "Conflicts with the Summon keeper hotkey.".to_owned())
    })
}

/// Validate a recording accelerator before touching registration (Story 20.4):
/// the empty string is rejected — clearing is a separate command
/// (`recording_hotkey_clear`), never an empty `set` — and anything else must
/// parse. Factored out of [`recording_hotkey_set`] so the reject paths are
/// unit-testable without an app handle.
#[cfg(desktop)]
fn validate_recording_hotkey(
    accelerator: &str,
) -> Result<tauri_plugin_global_shortcut::Shortcut, IpcError> {
    if accelerator.is_empty() {
        return Err(to_ipc_error(CoreError::Internal(
            "an empty accelerator cannot be assigned; use recording_hotkey_clear to unset"
                .to_owned(),
        )));
    }
    crate::hotkey::parse(accelerator).ok_or_else(|| {
        to_ipc_error(CoreError::Internal(format!(
            "invalid accelerator: {accelerator}"
        )))
    })
}

/// Build the [`HotkeyVm`] for the recording binding (Story 20.4): `isDefault`
/// = the empty (unset-by-default) string, `active` = a non-empty accelerator
/// that both parses AND is currently registered with the OS, and the soft
/// `conflict` from [`recording_hotkey_conflict`] (curated system shortcuts +
/// the cross-summon clash). Reuses the summon [`HotkeyVm`] shape.
#[cfg(desktop)]
fn recording_hotkey_vm(app: &tauri::AppHandle, accelerator: String, summon: &str) -> HotkeyVm {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let is_default = accelerator.is_empty();
    let active = !accelerator.is_empty()
        && crate::hotkey::parse(&accelerator)
            .map(|shortcut| app.global_shortcut().is_registered(shortcut))
            .unwrap_or(false);
    let conflict = recording_hotkey_conflict(&accelerator, summon);
    HotkeyVm {
        accelerator,
        is_default,
        active,
        conflict,
    }
}

/// Read the optional OS-global Start/Stop Recording hotkey binding (Story 20.4,
/// FR-50). Returns the persisted accelerator (absent ⇒ the empty string =
/// **unset**, the shipped default), whether it is unset (`isDefault`), whether it
/// is currently registered with the OS (`active`), and any soft conflict warning
/// including a clash with the summon binding. Failures funnel through
/// [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn recording_hotkey_get(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<HotkeyVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let accelerator =
        keeper_core::registry::get_recording_hotkey(&data_dir).map_err(to_ipc_error)?;
    let summon = keeper_core::registry::get_global_hotkey(&data_dir).map_err(to_ipc_error)?;
    Ok(recording_hotkey_vm(&app, accelerator, &summon))
}

/// Mobile stub for [`recording_hotkey_get`]: there is no OS-global hotkey (and
/// no recording) on iOS — an honest `Unsupported` through `to_ipc_error`. The
/// `recording` capability is reported `false`, so the row never renders.
#[cfg(not(desktop))]
#[tauri::command]
pub fn recording_hotkey_get() -> Result<HotkeyVm, IpcError> {
    Err(to_ipc_error(CoreError::Unsupported(
        "the OS-global recording hotkey is desktop-only".to_owned(),
    )))
}

/// Assign the OS-global Start/Stop Recording hotkey (Story 20.4, FR-50),
/// mirroring [`hotkey_set`]'s exact validate → unregister-old → register-new →
/// persist → rollback-on-any-failure discipline for the **independent**
/// `hotkey.recording` binding (the summon binding is never touched). An empty
/// accelerator is rejected — clearing is [`recording_hotkey_clear`]. A malformed
/// accelerator is rejected before registration; if the OS refuses the new
/// registration — or accepts it but persisting fails — the previous binding is
/// restored and nothing is persisted. Logs carry accelerator strings only.
#[cfg(desktop)]
#[tauri::command]
pub fn recording_hotkey_set(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    accelerator: String,
) -> Result<HotkeyVm, IpcError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;

    // Validate before touching registration (empty and malformed → reject).
    let new_shortcut = validate_recording_hotkey(&accelerator)?;

    let previous = keeper_core::registry::get_recording_hotkey(&data_dir).map_err(to_ipc_error)?;
    let summon = keeper_core::registry::get_global_hotkey(&data_dir).map_err(to_ipc_error)?;
    let gs = app.global_shortcut();

    // Unregister the currently-bound recording accelerator (best-effort — it may
    // already be gone if startup registration failed, or be unset entirely).
    if let Some(prev_shortcut) = crate::hotkey::recording_shortcut(&previous) {
        if gs.is_registered(prev_shortcut) {
            if let Err(error) = gs.unregister(prev_shortcut) {
                tracing::warn!(%error, accelerator = %previous, "hotkey: could not unregister old recording binding");
            }
        }
    }

    // Register the new accelerator with the recording press handler. A hard
    // failure restores the OLD binding and returns Err — nothing is persisted.
    if let Err(error) = gs.on_shortcut(new_shortcut, crate::hotkey::on_recording_shortcut_event) {
        tracing::warn!(%error, accelerator, "hotkey: OS refused to register recording binding; restoring previous");
        if let Some(prev_shortcut) = crate::hotkey::recording_shortcut(&previous) {
            if let Err(restore_error) =
                gs.on_shortcut(prev_shortcut, crate::hotkey::on_recording_shortcut_event)
            {
                tracing::warn!(%restore_error, accelerator = %previous, "hotkey: could not restore previous recording binding after a failed reassignment");
            }
        }
        return Err(to_ipc_error(CoreError::Internal(format!(
            "the system refused to register {accelerator}: {error}"
        ))));
    }

    // Only persist an accelerator the OS accepted; on a persist failure roll the
    // registration back to `previous` so the live OS state and the stored value
    // never diverge (the same discipline as `hotkey_set`).
    if let Err(error) = keeper_core::registry::set_recording_hotkey(&data_dir, &accelerator) {
        tracing::warn!(%error, accelerator, "hotkey: could not persist recording binding; rolling back to previous");
        if gs.is_registered(new_shortcut) {
            if let Err(unreg_error) = gs.unregister(new_shortcut) {
                tracing::warn!(%unreg_error, accelerator, "hotkey: could not unregister recording binding during rollback");
            }
        }
        if let Some(prev_shortcut) = crate::hotkey::recording_shortcut(&previous) {
            if let Err(restore_error) =
                gs.on_shortcut(prev_shortcut, crate::hotkey::on_recording_shortcut_event)
            {
                tracing::warn!(%restore_error, accelerator = %previous, "hotkey: could not restore previous recording binding after a failed persist");
            }
        }
        return Err(to_ipc_error(error));
    }
    Ok(recording_hotkey_vm(&app, accelerator, &summon))
}

/// Mobile stub for [`recording_hotkey_set`]: desktop-only — an honest
/// `Unsupported`. Nothing is validated, registered, or persisted.
#[cfg(not(desktop))]
#[tauri::command]
pub fn recording_hotkey_set(accelerator: String) -> Result<HotkeyVm, IpcError> {
    let _ = accelerator;
    Err(to_ipc_error(CoreError::Unsupported(
        "the OS-global recording hotkey is desktop-only".to_owned(),
    )))
}

/// Clear the OS-global Start/Stop Recording hotkey back to unset (Story 20.4):
/// unregister the current recording binding (best-effort — it may never have
/// registered) and persist the empty string. The returned VM reports the unset
/// state (`accelerator: ""`, `isDefault: true`, `active: false`). A persist
/// failure funnels through [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn recording_hotkey_clear(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<HotkeyVm, IpcError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let previous = keeper_core::registry::get_recording_hotkey(&data_dir).map_err(to_ipc_error)?;
    let summon = keeper_core::registry::get_global_hotkey(&data_dir).map_err(to_ipc_error)?;
    let gs = app.global_shortcut();
    if let Some(prev_shortcut) = crate::hotkey::recording_shortcut(&previous) {
        if gs.is_registered(prev_shortcut) {
            if let Err(error) = gs.unregister(prev_shortcut) {
                tracing::warn!(%error, accelerator = %previous, "hotkey: could not unregister recording binding on clear");
            }
        }
    }
    keeper_core::registry::set_recording_hotkey(&data_dir, "").map_err(to_ipc_error)?;
    Ok(recording_hotkey_vm(&app, String::new(), &summon))
}

/// Mobile stub for [`recording_hotkey_clear`]: desktop-only — an honest
/// `Unsupported`. Nothing is unregistered or persisted.
#[cfg(not(desktop))]
#[tauri::command]
pub fn recording_hotkey_clear() -> Result<HotkeyVm, IpcError> {
    Err(to_ipc_error(CoreError::Unsupported(
        "the OS-global recording hotkey is desktop-only".to_owned(),
    )))
}

/// The nearest existing ancestor of `path`, including `path` itself (Story
/// 20.4): the palette's "Open Recordings Folder" reveals the effective
/// destination even before the first session ever created it — falling back up
/// the tree (ultimately to `/`, which always exists) rather than erroring on a
/// not-yet-created folder. Pure over the filesystem probe.
#[cfg(desktop)]
fn nearest_existing_ancestor(path: &Path) -> PathBuf {
    // Require a real directory, not merely an existing path: `reveal_item_in_dir`
    // expects a folder, so an ancestor that resolves to a regular file (a
    // misconfigured destination nested under a file) must be skipped in favour of
    // the first true directory above it. Root always exists as a directory, so
    // `find` succeeds in practice; the fallback keeps the fn total.
    path.ancestors()
        .find(|candidate| candidate.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
}

/// Reveal the **effective** recordings destination folder in the OS file
/// manager (Story 20.4, FR-48) — the palette "Open Recordings Folder" verb.
/// Resolves the same [`effective_destination_dir`] source of truth
/// `recording_start` uses (persisted choice or the `~/Movies/keeper` default)
/// and reveals it — or its nearest existing ancestor when the folder has not
/// been created yet — via the opener plugin (the same
/// `reveal_item_in_dir` seam as [`reveal_path`] and the tray). A reveal failure
/// maps to a funnelled [`IpcError`], never a panic.
#[cfg(desktop)]
#[tauri::command]
pub fn recording_reveal_folder(state: State<'_, AppState>) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let destination = effective_destination_dir(&data_dir, &state.platform);
    let target = nearest_existing_ancestor(&destination);
    tauri_plugin_opener::reveal_item_in_dir(&target).map_err(|e| {
        to_ipc_error(CoreError::Internal(format!(
            "could not reveal the recordings folder: {e}"
        )))
    })
}

/// Mobile stub for [`recording_reveal_folder`]: there is no user-visible file
/// manager (and no recording) on iOS — an honest `Unsupported` through
/// [`to_ipc_error`].
#[cfg(not(desktop))]
#[tauri::command]
pub fn recording_reveal_folder() -> Result<(), IpcError> {
    Err(to_ipc_error(CoreError::Unsupported(
        "revealing the recordings folder is desktop-only".to_owned(),
    )))
}

/// Cancel a held send by its `id` (Story 8.3): delete the `outbox` row, persist its
/// body as the Chat's Draft, and return the restored body so the composer can restore
/// it. Performs **zero** network dispatch. Cancel of an already-dispatched/absent row
/// is an idempotent no-op that resolves with an empty string. Failures funnel through
/// [`to_ipc_error`]. The body is never logged.
#[tauri::command]
pub async fn cancel_held_send(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    id: String,
) -> Result<String, IpcError> {
    state
        .accounts
        .cancel_held_send(&state.platform, &account_id, &room_id, &id)
        .await
        .map_err(to_ipc_error)
}

/// Subscribe to the held sends for one open Chat (Story 8.3). Reuses the account's live
/// session (activating it lazily) and streams [`OutboxVm`] snapshots over `channel` — an
/// initial snapshot first, then a fresh full snapshot on every outbox change — returning
/// the subscription id. The sink forwards each snapshot to the channel; a closed channel
/// simply stops the producer. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn subscribe_outbox(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    channel: Channel<OutboxVm>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |snapshot: OutboxVm| channel.send(snapshot).is_ok());
    state
        .accounts
        .subscribe_outbox(&state.platform, &account_id, &room_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one outbox subscription, aborting its producer task (Story 8.3).
/// Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn unsubscribe_outbox(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_outbox(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Send a plain-text reply to a message through the single dispatch gate (FR-10,
/// FR-41, AD-13, Story 3.4). `inReplyToKey` is the *original* message's opaque
/// render `key` (its `unique_id`); the Rust core resolves it to the event id and
/// enqueues the reply. The reply's local echo and send-state transitions arrive
/// back over the existing timeline subscription (no echo is synthesized). A
/// missing target / enqueue failure funnels through [`to_ipc_error`] to
/// `SendFailed`.
#[tauri::command]
pub async fn send_reply(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    in_reply_to_key: String,
    body: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .send_reply(&account_id, &room_id, &in_reply_to_key, &body)
        .await
        .map_err(to_ipc_error)
}

/// Edit an own text message in place through the single dispatch gate (FR-11,
/// FR-41, AD-13, Story 3.4). `itemKey` is the message's opaque render `key` (its
/// `unique_id`); the Rust core resolves it, gates on editability (own + text), and
/// enqueues the edit. The `Set` diff that updates the content in place (and flips
/// `isEdited`) arrives back over the existing timeline subscription. A missing
/// target / non-editable message / enqueue failure funnels through
/// [`to_ipc_error`] to `SendFailed`.
#[tauri::command]
pub async fn edit_message(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    item_key: String,
    body: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .edit_message(&account_id, &room_id, &item_key, &body)
        .await
        .map_err(to_ipc_error)
}

/// Toggle the account's emoji reaction on a message through the single dispatch
/// gate (FR-12, FR-41, AD-13, Story 3.5). `itemKey` is the message's opaque render
/// `key` (its `unique_id`); the Rust core resolves it to the SDK
/// `TimelineEventItemId` and calls `Timeline::toggle_reaction` — adding the
/// reaction if absent, retracting it if the account already reacted with `emoji`.
/// The updated reaction set arrives back over the existing timeline subscription
/// as a `Set` diff (no state is synthesized). A missing target funnels through
/// [`to_ipc_error`] to a non-retriable `SendFailed`; an SDK dispatch failure to a
/// retriable `SendFailed`.
#[tauri::command]
pub async fn toggle_reaction(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    item_key: String,
    emoji: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .toggle_reaction(&account_id, &room_id, &item_key, &emoji)
        .await
        .map_err(to_ipc_error)
}

/// Resolve a search hit's `event_id` to the opaque timeline render key so the
/// frontend can deep-link into a room at the matched message (Story 5.4, FR-34).
/// `eventId` is the sanctioned deep-link handle returned on `SearchHitVm`; the
/// Rust core parses it and scans the room's live `Timeline` for the loaded item
/// whose event id matches, returning its opaque `unique_id` — `event_id` is an
/// input only, so no event id is ever added to a streamed timeline VM (the
/// `TimelineItemVm` no-event-id invariant, NFR-9/AD-1, holds). Resolves with the
/// render key when the event is a currently-loaded timeline item, else `null`
/// (the caller best-effort paginates + retries, or degrades honestly).
/// `Option<String>` serializes to `string | null` across IPC. An unparsable
/// room/event id funnels through [`to_ipc_error`] to `TimelineUnavailable` (never
/// a panic).
#[tauri::command]
pub async fn resolve_timeline_event_key(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    event_id: String,
) -> Result<Option<String>, IpcError> {
    state
        .accounts
        .resolve_timeline_event_key(&account_id, &room_id, &event_id)
        .await
        .map_err(to_ipc_error)
}

/// Delete an own message for everyone by issuing a Matrix redaction through the
/// single dispatch gate (FR-15, FR-41, AD-13, Story 3.8). `itemKey` is the
/// message's opaque render `key` (its `unique_id`); the Rust core resolves it to
/// the SDK `TimelineEventItemId` and calls `Timeline::redact` with no reason
/// (`None`). The `Set` diff that turns the message into a redacted stub in place
/// arrives back over the existing timeline subscription (nothing is synthesized).
/// A missing target funnels through [`to_ipc_error`] to a non-retriable
/// `SendFailed`; an SDK dispatch failure to a retriable `SendFailed`.
#[tauri::command]
pub async fn delete_message(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    item_key: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .redact_message(&account_id, &room_id, &item_key, None)
        .await
        .map_err(to_ipc_error)
}

/// Resolve the bridged-Chat Network label for the delete confirmation on demand
/// (FR-15, UX-DR17, Story 3.8). Delegates to the core, which reads the Room's
/// MSC2346 `m.bridge` (and legacy `uk.half-shot.bridge`) state event and returns
/// the Network's display name ("Telegram", "WhatsApp", …), or `None` for a native
/// Matrix Room (no bridge state). `Option<String>` serializes to `string | null`
/// across IPC — only the resolved, non-secret label crosses. An unknown
/// room/account funnels through [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn room_network_label(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<Option<String>, IpcError> {
    state
        .accounts
        .room_network_label(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Subscribe to an account's connection status (FR-8/FR-9, UX-DR18, AD-8).
///
/// Lazily activates the account (reusing the room-list/timeline path), then
/// streams [`ConnectionStatusBatch`]es over `channel` — an initial snapshot of
/// the current status, then deduped changes — and returns the subscription id.
/// The sink forwards each batch to the channel; a closed channel simply drops
/// the batch (the frontend has unsubscribed). An activation failure funnels
/// through [`to_ipc_error`] to the existing `SyncUnavailable` code.
#[tauri::command]
pub async fn connection_status_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<ConnectionStatusBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: ConnectionStatusBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_connection_status(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one connection-status subscription, aborting its producer
/// task (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn connection_status_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_connection_status(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Subscribe to live remote draft edits across every account (Story 7.2, AD-15).
///
/// App-wide (not per account): streams a [`DraftMirrorBatch`] over `channel` for each
/// `dev.keeper.draft` room-account-data edit observed by any account's handler, and
/// returns the subscription id. The frontend pumps these into the drafts store's
/// `remote` map for local-wins conflict detection. The sink forwards each batch to the
/// channel; a closed channel drops the batch (the relay then stops). Never fails — the
/// relay is spawned unconditionally.
#[tauri::command]
pub async fn draft_mirror_subscribe(
    state: State<'_, AppState>,
    channel: Channel<DraftMirrorBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: DraftMirrorBatch| channel.send(batch).is_ok());
    Ok(state.accounts.subscribe_draft_mirror(sink).await)
}

/// Unsubscribe exactly one draft-mirror subscription, aborting its relay task (Story
/// 7.2). Idempotent — unsubscribing an unknown id is a no-op.
#[tauri::command]
pub async fn draft_mirror_unsubscribe(
    state: State<'_, AppState>,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_draft_mirror(subscription_id)
        .await;
    Ok(())
}

/// Subscribe to an account's encryption (device-verification) status (Story 3.1,
/// AD-8).
///
/// Lazily activates the account (reusing the room-list/timeline/connection path),
/// then streams [`EncryptionStatusBatch`]es over `channel` — an initial snapshot
/// of the current status, then deduped changes — and returns the subscription id.
/// The sink forwards each batch to the channel; a closed channel simply drops the
/// batch (the frontend has unsubscribed). An activation failure funnels through
/// [`to_ipc_error`] to the existing `SyncUnavailable` code.
#[tauri::command]
pub async fn encryption_status_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<EncryptionStatusBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: EncryptionStatusBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_encryption_status(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one encryption-status subscription, aborting its producer
/// task (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn encryption_status_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_encryption_status(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Subscribe to an account's interactive device self-verification flow (Story
/// 3.2, FR-14, AD-1, AD-8).
///
/// Lazily activates the account, then streams [`VerificationFlowVm`] snapshots
/// over `channel` — the flow's state machine (waiting → compare emoji / show QR →
/// confirmed → done/cancelled/failed). An *incoming* request (the peer started it)
/// surfaces here as a `Requested` snapshot so the UI can auto-open the modal. The
/// sink forwards each snapshot to the channel; a closed channel drops the snapshot
/// (the frontend unsubscribed). NO `Verification`/SAS/QR object, key, or plaintext
/// crosses IPC — only the rendered VM. Activation failure funnels through
/// [`to_ipc_error`] to `SyncUnavailable`.
#[tauri::command]
pub async fn verification_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<VerificationFlowVm>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |vm: VerificationFlowVm| channel.send(vm).is_ok());
    state
        .accounts
        .subscribe_verification(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one verification subscription, aborting its producer task
/// and clearing the account's flow sender (AD-19). Idempotent.
#[tauri::command]
pub async fn verification_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_verification(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Start an interactive self-verification from keeper against the user's other
/// session (Story 3.2, FR-14). Requests the verification in Rust and feeds the new
/// flow id into the live verification producer so it streams over the existing
/// verification subscription. Requires an active verification subscription.
/// Failures funnel through [`to_ipc_error`] to `VerificationFailed`.
#[tauri::command]
pub async fn verification_start(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_start(&account_id)
        .await
        .map_err(to_ipc_error)
}

/// Accept an incoming verification request the peer started (Story 3.2). Moves the
/// flow from `Requested` to `Ready`. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn verification_accept(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_accept(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Start the emoji/SAS sub-flow on a ready request (Story 3.2). The SAS state
/// transition arrives over the verification stream. Failures funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub async fn verification_start_sas(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_start_sas(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Confirm the SAS emoji match on our side (Story 3.2). On both sides confirming,
/// the SDK completes verification and 3.1's `verification_state()` stream flips the
/// account to `Verified`. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn verification_confirm(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_confirm(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Signal that the SAS emoji do NOT match (Story 3.2). Cancels the flow with the
/// SDK mismatch code, which surfaces as `Failed`. Failures funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub async fn verification_mismatch(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_mismatch(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Cancel the verification flow (Story 3.2) — the user closed the modal / pressed
/// Esc. Cancels the active SAS or the request; a missing flow is a no-op. Failures
/// funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn verification_cancel(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_cancel(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Subscribe to an account's server-side key-backup status (Story 3.3, FR-14,
/// AD-8).
///
/// Lazily activates the account (reusing the shared session path), then streams
/// [`BackupStatus`] snapshots over `channel` — an initial snapshot of the current
/// status, then deduped changes — and returns the subscription id. The sink
/// forwards each status to the channel; a closed channel drops the status (the
/// frontend unsubscribed). NO recovery key or secret-storage material crosses IPC
/// — only the enum tag. An activation failure funnels through [`to_ipc_error`] to
/// the existing `SyncUnavailable` code.
#[tauri::command]
pub async fn backup_status_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<BackupStatus>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |status: BackupStatus| channel.send(status).is_ok());
    state
        .accounts
        .subscribe_backup_status(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one backup-status subscription, aborting its backend
/// producer task (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn backup_status_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_backup_status(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Enable server-side key backup for the account (Story 3.3, FR-14). Delegates to
/// the core, which creates the backup + secret store and returns the base58
/// **recovery key** *once* — the deliberate boundary exception, meant for the
/// human to save (shown once in `mono`). A race with an existing server backup
/// funnels through [`to_ipc_error`] to the named `backupExists` code so the modal
/// can offer restore; any other failure maps to `backupFailed`. The recovery key
/// is never logged.
#[tauri::command]
pub async fn backup_enable(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<String, IpcError> {
    state
        .accounts
        .backup_enable(&account_id)
        .await
        .map_err(to_ipc_error)
}

/// Restore from server-side key backup with a recovery key (Story 3.3, FR-14).
/// Delegates to the core, which opens the secret store and imports secrets; the
/// SDK then downloads room keys automatically, so 3.1's streams re-render
/// previously-undecryptable rows with no extra code. An invalid key funnels
/// through [`to_ipc_error`] to a *named* code (`backupMalformedKey` vs
/// `backupIncorrectKey`), never a generic failure. The recovery key is never
/// logged.
#[tauri::command]
pub async fn backup_restore(
    state: State<'_, AppState>,
    account_id: String,
    recovery_key: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .backup_restore(&account_id, &recovery_key)
        .await
        .map_err(to_ipc_error)
}

/// Save a recovery key to the OS Keychain (Story 3.3, FR-14) — the user's opt-in
/// after seeing the key once. Delegates to the core, which writes it at
/// `recovery_key/<account_id>` via the [`Platform`] keychain port. A write
/// failure funnels through [`to_ipc_error`] so the modal can keep the key visible
/// for manual copy. The recovery key is never logged.
#[tauri::command]
pub async fn backup_save_recovery_key(
    state: State<'_, AppState>,
    account_id: String,
    recovery_key: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .backup_save_recovery_key(&state.platform, &account_id, &recovery_key)
        .await
        .map_err(to_ipc_error)
}

/// Read a previously-saved recovery key from the OS Keychain (Story 3.3) to
/// prefill the restore textarea, or `None` if none was saved. `Option<String>`
/// serializes to `string | null` across IPC. Failures funnel through
/// [`to_ipc_error`]. The recovery key is never logged.
#[tauri::command]
pub async fn backup_saved_recovery_key(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<String>, IpcError> {
    state
        .accounts
        .backup_saved_recovery_key(&state.platform, &account_id)
        .await
        .map_err(to_ipc_error)
}

/// Retry a failed outgoing message by re-driving its wedged local echo through
/// the controlled send path (`unwedge`, not a new dispatch — FR-41). `item_key`
/// is the timeline item's opaque `unique_id`. A missing echo / no open timeline
/// funnels through [`to_ipc_error`] to `SendFailed`.
#[tauri::command]
pub async fn send_retry(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    item_key: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .retry_send(&account_id, &room_id, &item_key)
        .await
        .map_err(to_ipc_error)
}

/// Send a media attachment from an OS file path through the single dispatch gate
/// (FR-13, FR-41, AD-4, AD-13, Story 3.7). The composer attach button and native
/// drag-drop both deliver a **path** — Rust reads the file itself, so no media
/// bytes cross IPC. `caption` is the trimmed composer text (`None` when empty). The
/// local echo + every send-state transition arrive back over the existing timeline
/// subscription (no echo is synthesized). An enqueue-time failure funnels through
/// [`to_ipc_error`] to `SendFailed`.
#[tauri::command]
pub async fn send_attachment_path(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    path: String,
    caption: Option<String>,
) -> Result<(), IpcError> {
    state
        .accounts
        .send_attachment_path(
            &account_id,
            &room_id,
            std::path::Path::new(&path),
            caption.as_deref(),
        )
        .await
        .map_err(to_ipc_error)
}

/// Send a path-less pasted clipboard image through the single dispatch gate (FR-13,
/// FR-41, AD-4, AD-13, Story 3.7). The image **bytes** ride as a **raw binary IPC
/// body** (`InvokeBody::Raw`, ~1× size, never base64/JSON) — the sanctioned
/// exception for pastes with no OS path — with `accountId`/`roomId`/`filename`/
/// `mime`/`caption` carried in **request headers** (filename + caption are
/// percent-encoded so non-ASCII survives an ASCII-only header). Rust reads the raw
/// body, decodes the headers, and enqueues the attachment; the local echo +
/// send-state transitions arrive over the existing timeline subscription. A missing
/// required header, or an enqueue-time failure, funnels through [`to_ipc_error`] to
/// `SendFailed`.
#[tauri::command]
pub async fn send_attachment_bytes(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), IpcError> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(to_ipc_error(CoreError::Send(SendError::Upload(
            "pasted attachment must be sent as a raw binary body".to_owned(),
        ))));
    };
    let bytes = bytes.clone();
    let headers = request.headers();
    let account_id = required_header(headers, "x-account-id")?;
    let room_id = required_header(headers, "x-room-id")?;
    // Filename + caption are percent-encoded by the caller so non-ASCII survives an
    // ASCII-only header value.
    let filename =
        decode_header(headers, "x-filename").unwrap_or_else(|| "pasted-image".to_owned());
    let mime = required_header(headers, "x-mime")?;
    let caption = decode_header(headers, "x-caption");
    state
        .accounts
        .send_attachment_bytes(
            &account_id,
            &room_id,
            bytes,
            &filename,
            &mime,
            caption.as_deref(),
        )
        .await
        .map_err(to_ipc_error)
}

/// Cancel an in-flight outgoing echo by aborting its SDK send handle (best-effort,
/// Story 3.7). `item_key` is the echo's opaque `unique_id`. If the send already
/// dispatched, the abort is a no-op and the message stays sent (the echo's removal
/// or its no-op arrives over the existing timeline subscription). A missing echo /
/// no open timeline funnels through [`to_ipc_error`] to `SendFailed`.
#[tauri::command]
pub async fn cancel_send(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    item_key: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .cancel_send(&account_id, &room_id, &item_key)
        .await
        .map_err(to_ipc_error)
}

/// Mark a room read (Story 3.9 receipts, Story 4.1, AD-14). Delegates to the core,
/// which dispatches a public `m.read` receipt on the room's latest event through
/// the receipt/typing signals seam — other Matrix clients observe the advance — and
/// clears any manual `m.marked_unread` flag. Works for any inbox row whether or not
/// its timeline is open. Best-effort: a dispatch failure is logged and swallowed in
/// the core (no UI error), so this resolves `Ok` even then. A room-not-found /
/// inactive account funnels through [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn mark_room_read(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .mark_room_read(&state.platform, &account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Kick every live account's sync loop (Story 13.6): the phone pull-to-refresh
/// and the global "Sync now" palette/menu action. Delegates to the core, which
/// resumes each already-active account's `SyncService` via its idempotent
/// `start()` — the same resume operation Epic 14-1's foreground wake will route
/// through. It never builds a second sync loop and never activates signed-out
/// accounts. Best-effort and infallible: `start()` cannot fail and an empty
/// account set is a no-op, so this never returns an error in practice.
#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.accounts.sync_now().await;
    Ok(())
}

/// Query the command palette (Story 9.1, epic 9 spine). Serves grouped, ranked,
/// bounded results from the in-memory Rust index over **every** room across all
/// accounts (chats + DM contacts) plus the static action registry — the frontend
/// only renders and dispatches by id, never filters or re-orders (AD-20).
///
/// `mode` picks the query mode: `default` filters chats + contacts (≥2 chars) plus
/// matching actions; `action` (the `>` prefix) returns only actions with open-chat
/// actions ranked first when `openChat` is set. On an empty/short/no-match query
/// the top registered actions are returned so the frontend can show them plus a `>`
/// hint. Never fails (an empty index simply yields the global actions); the < 100 ms
/// budget at 10k chats is met by the pure linear scan in `keeper_core::palette`.
#[tauri::command]
pub async fn palette_query(
    state: State<'_, AppState>,
    query: String,
    mode: PaletteMode,
    open_chat: bool,
) -> Result<PaletteResultsVm, IpcError> {
    // The recording capability gates the `open-recording` action out of the palette
    // (and thus the cheat sheet + native menu) when unavailable (Story 16.3); the
    // notes capability does the same for the whole Notes section (FR-122, AD-27),
    // and `bots` for the Bots section (Epic 61, FR-384). The bots flag is spelled
    // here exactly as `capabilities` spells it — `cfg!(desktop)` — rather than
    // borrowed from `notes`, because a desktop build with folder sync off still
    // has a Bots pane.
    let recording = crate::macos_version::recording_supported();
    let notes = notes_available(&state);
    Ok(state
        .accounts
        .palette_query(&query, mode, open_chat, recording, notes, cfg!(desktop))
        .await)
}

/// Return the category-grouped, toggle-collapsed registry projection (Story 9.3).
///
/// The data source for the ⌘? cheat sheet: a pure projection of the same
/// `palette_actions()` registry the palette consumes, grouped by category and with
/// each toggle pair collapsed to one row (`keeper_core::palette::registry_sections`).
/// The native macOS menu bar is built from this same projection in Rust, so the two
/// discovery surfaces provably never drift (UX-DR15). Pure and stateless — never
/// fails.
#[tauri::command]
pub fn cheat_sheet_sections(state: State<'_, AppState>) -> Result<Vec<MenuSectionVm>, IpcError> {
    // Gate the recording action out of the cheat sheet when the capability is off
    // (Story 16.3), keeping it in lockstep with the palette and native menu. The
    // notes gate rides the same mechanism (Story 36.2): six actions declared once
    // reach the palette, the ⌘? sheet, the native menu bar and the tray, so the
    // four cannot drift (UX-DR42). The bots gate is its own (Epic 61, FR-384) and
    // is spelled the way `capabilities` spells it.
    Ok(keeper_core::palette::registry_sections(
        crate::macos_version::recording_supported(),
        notes_available(&state),
        cfg!(desktop),
    ))
}

/// Read the resolved Incognito state for `(accountId, roomId)` (Story 8.1). Delegates
/// to the core, which reads the three registry scopes and applies the `signals`
/// Chat > Account > Global resolver, returning an [`IncognitoVm`] the frontend renders
/// directly — precedence is never resolved on the frontend. Errors funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub fn incognito_get(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<IncognitoVm, IpcError> {
    state
        .accounts
        .incognito_get(&state.platform, &account_id, &room_id)
        .map_err(to_ipc_error)
}

/// Read the "message previews" toggle (Story 10.1). Returns the in-memory
/// [`NotifyConfig`](keeper_core::notify::NotifyConfig) value (seeded from the persisted
/// registry at startup; default on). Infallible — reads process state, never fails.
#[tauri::command]
pub fn notify_get_preview_enabled(state: State<'_, AppState>) -> Result<bool, IpcError> {
    Ok(state.accounts.notify_previews_get())
}

/// Set the "message previews" toggle (Story 10.1). Persists into the `settings` k/v
/// table under `notify.previews_enabled` and updates the in-memory config so every live
/// notify handler sees the change immediately. Errors funnel through [`to_ipc_error`].
#[tauri::command]
pub fn notify_set_preview_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), IpcError> {
    state
        .accounts
        .notify_previews_set(&state.platform, enabled)
        .map_err(to_ipc_error)
}

/// Read the global Do-Not-Disturb switch (Story 10.2). Returns the in-memory
/// [`NotifyConfig`](keeper_core::notify::NotifyConfig) value (seeded from the persisted
/// registry at startup; default off). Infallible — reads process state, never fails.
#[tauri::command]
pub fn dnd_get_global(state: State<'_, AppState>) -> Result<bool, IpcError> {
    Ok(state.accounts.dnd_get())
}

/// Set the global Do-Not-Disturb switch (Story 10.2). Persists into the `settings` k/v
/// table under `notify.dnd_global` and updates the in-memory config so every live notify
/// handler sees the change immediately. Errors funnel through [`to_ipc_error`].
#[tauri::command]
pub fn dnd_set_global(state: State<'_, AppState>, enabled: bool) -> Result<(), IpcError> {
    state
        .accounts
        .dnd_set(&state.platform, enabled)
        .map_err(to_ipc_error)
}

/// Read whether a Network label is currently muted (Story 10.2). Reads the persisted
/// `muted_networks` table. Errors funnel through [`to_ipc_error`].
#[tauri::command]
pub fn network_mute_get(state: State<'_, AppState>, network_id: String) -> Result<bool, IpcError> {
    state
        .accounts
        .network_mute_get(&state.platform, &network_id)
        .map_err(to_ipc_error)
}

/// Set (or clear) the muted state for a Network label (Story 10.2). Persists into the
/// `muted_networks` table and updates the in-memory config so every live notify handler
/// and the inbox glyph see the change immediately. Errors funnel through [`to_ipc_error`].
#[tauri::command]
pub fn network_mute_set(
    state: State<'_, AppState>,
    network_id: String,
    muted: bool,
) -> Result<(), IpcError> {
    state
        .accounts
        .network_mute_set(&state.platform, &network_id, muted)
        .map_err(to_ipc_error)
}

/// Read the dock-badge mode (Story 10.3, FR-53). Returns the in-memory
/// [`BadgeConfig`](keeper_core::badge::BadgeConfig) value (seeded from the persisted
/// registry at startup; default `all`). Infallible — reads process state, never fails.
#[tauri::command]
pub fn dock_badge_mode_get(state: State<'_, AppState>) -> Result<DockBadgeMode, IpcError> {
    Ok(state.accounts.dock_badge_mode_get())
}

/// Set the dock-badge mode (Story 10.3, FR-53). Persists into the `settings` k/v table
/// under `notify.dock_badge_mode`, updates the in-memory config, and re-pokes the live
/// inbox merger so the badge is recomputed and reapplied immediately. Errors funnel
/// through [`to_ipc_error`].
#[tauri::command]
pub async fn dock_badge_mode_set(
    state: State<'_, AppState>,
    mode: DockBadgeMode,
) -> Result<(), IpcError> {
    state
        .accounts
        .dock_badge_mode_set(&state.platform, mode)
        .await
        .map_err(to_ipc_error)
}

/// Report the currently-visible Chat to the shared notify engine (Story 14.3, AD-18).
///
/// Both `Some` ⇒ set the active `(account_id, room_id)`; both `None` ⇒ clear it. A message
/// for exactly the active Chat is suppressed by `should_notify` (its content is already on
/// screen). Reported by the iOS shell from `roomsStore.selected` on the reduced tier only,
/// so desktop notification behavior is unchanged (desktop never invokes this). Ephemeral
/// process state, never persisted; infallible in practice.
#[tauri::command]
pub fn active_chat_set(
    state: State<'_, AppState>,
    account_id: Option<String>,
    room_id: Option<String>,
) -> Result<(), IpcError> {
    match (account_id, room_id) {
        (Some(account_id), Some(room_id)) => {
            state.accounts.set_active_room(&account_id, &room_id);
        }
        // Any incomplete pair (or both `None`) clears the active Chat — no partial state.
        _ => state.accounts.clear_active_room(),
    }
    Ok(())
}

/// Record the last phone-stack navigation level (Story 14.4). Reported by the iOS shell
/// on the reduced tier whenever a Chat is open (`detail_open` marks the level-2 Detail),
/// so a webview reload after a content-process jettison (tauri#14371) can land the user
/// exactly where they were. Nav *selection* only — never message/room data (AD-8).
/// Ephemeral process state, never persisted; infallible in practice.
#[tauri::command]
pub fn nav_state_set(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    detail_open: bool,
) -> Result<(), IpcError> {
    slot_set(
        &state.nav_state,
        NavState {
            account_id,
            room_id,
            detail_open,
        },
    );
    Ok(())
}

/// Clear the stored navigation level (Story 14.4) — the user returned to the Inbox, so
/// a later reload honestly starts at level 0. Idempotent; infallible in practice.
#[tauri::command]
pub fn nav_state_clear(state: State<'_, AppState>) -> Result<(), IpcError> {
    slot_take(&state.nav_state);
    Ok(())
}

/// Read the stored navigation level (Story 14.4), or `None` on a cold launch (a true
/// app kill restarts Rust fresh, so no stored nav ⇒ the Inbox). A read, not a take —
/// the shell keeps reporting over it, and a StrictMode effect re-run must never
/// consume the state out from under its sibling read. Infallible in practice.
#[tauri::command]
pub fn nav_state_get(state: State<'_, AppState>) -> Result<Option<NavState>, IpcError> {
    Ok(slot_get(&state.nav_state))
}

/// Read the OS notification-permission state (Story 14.3). Reaches the write-once
/// notification app handle and the plugin's `permission_state()`, mapping to the typed
/// [`NotificationPermission`] the iOS Settings surface reads. `Granted`/`Denied` mirror the
/// plugin; every other plugin state (prompt / prompt-with-rationale), an unset handle, or a
/// read error resolves to `Unknown` (the UI then hides the persistent "off" state rather
/// than guessing). Never re-prompts. Infallible — degrades to `Unknown` rather than erroring.
#[tauri::command]
pub fn notification_permission_state(
    _state: State<'_, AppState>,
) -> Result<NotificationPermission, IpcError> {
    use tauri::plugin::PermissionState;
    use tauri_plugin_notification::NotificationExt;

    let Some(app) = NOTIFY_APP.get() else {
        // Headless / pre-setup: no handle to read, so the state is unknown.
        return Ok(NotificationPermission::Unknown);
    };
    let permission = match app.notification().permission_state() {
        Ok(PermissionState::Granted) => NotificationPermission::Granted,
        Ok(PermissionState::Denied) => NotificationPermission::Denied,
        // Prompt / prompt-with-rationale / any future state: not a persistent "off".
        Ok(_) => NotificationPermission::Unknown,
        Err(error) => {
            tracing::warn!(%error, "notify: could not read permission state; treating as unknown");
            NotificationPermission::Unknown
        }
    };
    Ok(permission)
}

/// Open this app's page in the iOS system Settings (Story 14.3). Delegates to the Rust
/// opener (`Platform::open_url("app-settings:")`) so the deep link bypasses the opener JS
/// default scope (which only permits `mailto`/`tel`/`http(s)`). Used by the
/// permission-denied "Open Settings" affordance; never re-prompts. On desktop the opener
/// handles the URL through the OS as usual. Failures funnel through [`to_ipc_error`] but
/// the caller treats this best-effort (swallows rejection).
#[tauri::command]
pub fn ios_open_app_settings(state: State<'_, AppState>) -> Result<(), IpcError> {
    const IOS_APP_SETTINGS_URL: &str = "app-settings:";
    state
        .platform
        .open_url(IOS_APP_SETTINGS_URL)
        .map_err(to_ipc_error)
}

/// Resolve the live recording permission pre-flight (Story 16.5, FR-67, AD-36;
/// mic/camera legs Story 20.2). Runs the sidecar `getCapabilities` probe (a
/// fresh child `keeper-rec` per call — live detection, never a cached grant;
/// bounded by the shell's pre-flight timeout so a wedged sidecar resolves a
/// clean error) and resolves all three legs from that ONE probe (no new sidecar
/// RPC, `PROTOCOL_VERSION` stays 1): screen via the two-valued preflight lifted
/// with the session "already requested" flag, mic/camera via the direct
/// AVFoundation tri-state mapping — each `Some` only when the frontend reports
/// that source enabled. The probe never prompts. Called at Recording-view
/// render and re-called on every focus/return and enabled-source change.
/// Failures (sidecar unavailable / hung / iOS) funnel through [`to_ipc_error`];
/// the frontend swallows them to a safe default (Start disabled, no row claimed
/// granted) — never a crash, never an infinite spinner.
#[tauri::command]
pub async fn recording_permission(
    state: State<'_, AppState>,
    mic_enabled: bool,
    camera_enabled: bool,
) -> Result<RecordingPermissionVm, IpcError> {
    let capabilities = state
        .recorder
        .get_capabilities()
        .await
        .map_err(to_ipc_error)?;
    let requested = state.recording_permission_requested.load(Ordering::Relaxed);
    Ok(resolve_recording_permission(
        resolve_screen_recording_access(capabilities.screen_recording, requested),
        mic_enabled.then(|| resolve_source_access(capabilities.microphone)),
        camera_enabled.then(|| resolve_source_access(capabilities.camera)),
    ))
}

/// Request Screen Recording access through the sidecar (Story 16.5, FR-67,
/// AD-36): sets the session "already requested" flag, runs the
/// `requestScreenRecording` round-trip (`CGRequestScreenCaptureAccess` in the
/// child sidecar, so TCC shows keeper's own usage string and the OS posts its one
/// real prompt per app lifetime where allowed), and re-resolves the tri-state
/// from the reported outcome: granted ⇒ Start unlocks; not granted (a prior
/// denial shows no prompt at all) ⇒ denied-with-fix-path, and the row offers the
/// System Settings deep link. Story 20.2: the returned VM carries the mic/camera
/// legs too (resolved from a `getCapabilities` probe when a source is enabled),
/// so the adopted result never blanks an enabled source's row or its Start gate.
/// Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn request_screen_recording_permission(
    state: State<'_, AppState>,
    mic_enabled: bool,
    camera_enabled: bool,
) -> Result<RecordingPermissionVm, IpcError> {
    let granted = state
        .recorder
        .request_screen_recording()
        .await
        .map_err(to_ipc_error)?;
    // Latch the session "already requested" flag only after the round-trip actually
    // reached the sidecar (Ok ⇒ `CGRequestScreenCaptureAccess` ran, so the one real
    // OS prompt was posted/spent). If the round-trip errors (sidecar unavailable /
    // hung), no prompt was shown — leaving the flag clear keeps a later probe honest
    // as "not yet requested" rather than a spurious denied-with-fix-path.
    state
        .recording_permission_requested
        .store(true, Ordering::Relaxed);
    // Re-resolve through the same pure mapping the fetch path uses. The request
    // outcome IS the live OS answer: granted reads back as a green preflight;
    // not-granted stays two-valued undetermined, which the now-set session flag
    // resolves to the honest denied-with-fix-path.
    let preflight = if granted {
        TccPermission::Granted
    } else {
        TccPermission::NotDetermined
    };
    let screen = resolve_screen_recording_access(preflight, true);
    // The mic/camera legs (Story 20.2) come from a `getCapabilities` probe —
    // the `requestScreenRecording` round-trip reports only the screen outcome.
    // Probed (non-prompting) only when a source is actually enabled; with both
    // sources off this keeps the 16.5 single-round-trip path unchanged.
    let (microphone, camera) = if mic_enabled || camera_enabled {
        match state.recorder.get_capabilities().await {
            Ok(capabilities) => (
                mic_enabled.then(|| resolve_source_access(capabilities.microphone)),
                camera_enabled.then(|| resolve_source_access(capabilities.camera)),
            ),
            // The screen request already succeeded and its "already requested"
            // flag is latched; a failed leg probe must not discard that grant by
            // propagating the error and collapsing the whole request to the safe
            // default. Degrade the unconfirmed enabled legs to `NotYetRequested`
            // (Start stays honestly blocked on them, never falsely unlocked) — a
            // later live probe (focus/return or the enable re-sync) resolves them.
            Err(_) => (
                mic_enabled.then_some(ScreenRecordingAccess::NotYetRequested),
                camera_enabled.then_some(ScreenRecordingAccess::NotYetRequested),
            ),
        }
    } else {
        (None, None)
    };
    Ok(resolve_recording_permission(screen, microphone, camera))
}

/// Open the macOS System Settings pane for Screen Recording (Story 16.5, FR-67)
/// — the fix path for a denied grant, where re-prompting is impossible. Mirrors
/// [`ios_open_app_settings`]: the deep link goes through the Rust opener
/// (`Platform::open_url`) so it bypasses the opener JS default scope. Never
/// re-prompts. Failures funnel through [`to_ipc_error`] but the caller treats
/// this best-effort (swallows rejection).
#[tauri::command]
pub fn open_screen_recording_settings(state: State<'_, AppState>) -> Result<(), IpcError> {
    const SCREEN_RECORDING_SETTINGS_URL: &str =
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
    state
        .platform
        .open_url(SCREEN_RECORDING_SETTINGS_URL)
        .map_err(to_ipc_error)
}

/// Open the macOS System Settings pane for Microphone (Story 20.2, FR-67) —
/// the Microphone row's fix path for a denied grant, where re-prompting is
/// impossible. Mirrors [`open_screen_recording_settings`]: the deep link goes
/// through the Rust opener (`Platform::open_url`) so it bypasses the opener JS
/// default scope. Never re-prompts. Failures funnel through [`to_ipc_error`]
/// but the caller treats this best-effort (swallows rejection).
#[tauri::command]
pub fn open_microphone_settings(state: State<'_, AppState>) -> Result<(), IpcError> {
    const MICROPHONE_SETTINGS_URL: &str =
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
    state
        .platform
        .open_url(MICROPHONE_SETTINGS_URL)
        .map_err(to_ipc_error)
}

/// Open the macOS System Settings pane for Camera (Story 20.2, FR-67) — the
/// Camera row's fix path for a denied grant, where re-prompting is impossible.
/// Mirrors [`open_screen_recording_settings`]: the deep link goes through the
/// Rust opener (`Platform::open_url`) so it bypasses the opener JS default
/// scope. Never re-prompts. Failures funnel through [`to_ipc_error`] but the
/// caller treats this best-effort (swallows rejection).
#[tauri::command]
pub fn open_camera_settings(state: State<'_, AppState>) -> Result<(), IpcError> {
    const CAMERA_SETTINGS_URL: &str =
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera";
    state
        .platform
        .open_url(CAMERA_SETTINGS_URL)
        .map_err(to_ipc_error)
}

/// Request microphone access through the sidecar (Story 19.3, FR-69, AD-36):
/// runs the `requestMicrophone` round-trip (`AVCaptureDevice.requestAccess(for:
/// .audio)` in the child sidecar, so TCC shows keeper's own usage string —
/// `NSMicrophoneUsageDescription` in keeper's Info.plist — and the OS posts its
/// one real prompt per app lifetime where allowed) and resolves the authoritative
/// post-request [`TccPermission`] tri-state. Called lazily — only when the user
/// enables the mic source on the Audio card or hits the Microphone pre-flight
/// row's "Request permission" (Story 20.2), never preemptively (the setup
/// surface renders without probing; FR-69). Since Story 20.2 an enabled mic
/// that is not granted blocks Start — the pre-flight row surfaces the honest
/// tri-state and the fix path. Failures (sidecar unavailable / hung / iOS)
/// funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn request_microphone_permission(
    state: State<'_, AppState>,
) -> Result<TccPermission, IpcError> {
    state
        .recorder
        .request_microphone()
        .await
        .map_err(to_ipc_error)
}

/// Request camera access through the sidecar (Story 20.1, FR-70, AD-36):
/// runs the `requestCamera` round-trip (`AVCaptureDevice.requestAccess(for:
/// .video)` in the child sidecar, so TCC shows keeper's own usage string —
/// `NSCameraUsageDescription` in keeper's Info.plist — and the OS posts its
/// one real prompt per app lifetime where allowed) and resolves the
/// authoritative post-request [`TccPermission`] tri-state. Called lazily —
/// only when the user enables the Webcam switch or hits the Camera pre-flight
/// row's "Request permission" (Story 20.2), never preemptively. Since Story
/// 20.2 an enabled webcam that is not granted blocks Start — the pre-flight
/// row surfaces the honest tri-state and the fix path. Failures (sidecar
/// unavailable / hung / iOS) funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn request_camera_permission(
    state: State<'_, AppState>,
) -> Result<TccPermission, IpcError> {
    state.recorder.request_camera().await.map_err(to_ipc_error)
}

/// Project a [`SessionState`] into the UI-facing [`RecordingUiState`] (Story 16.6).
fn recording_ui_state(state: SessionState) -> RecordingUiState {
    match state {
        SessionState::Idle => RecordingUiState::Idle,
        SessionState::Preflight => RecordingUiState::Preflight,
        SessionState::Recording => RecordingUiState::Recording,
        SessionState::Rotating => RecordingUiState::Rotating,
        SessionState::Stopping => RecordingUiState::Stopping,
        SessionState::Finalized => RecordingUiState::Finalized,
        SessionState::Recovered => RecordingUiState::Recovered,
        SessionState::Failed => RecordingUiState::Failed,
    }
}

/// Lock a shared recording-status snapshot, recovering a poisoned lock (plain
/// data, no invariant a mid-write panic could break — never panic the app).
fn status_lock(status: &Mutex<RecordingStatusVm>) -> std::sync::MutexGuard<'_, RecordingStatusVm> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Fold one sidecar [`RecordingEvent`] through the platform-free machine into the
/// shared status snapshot, and fire the Story 18.4 loud-failure/warning native
/// notification **exactly on onset** — the driver sink's testable core
/// ([`RecordingSink`] adds the ledger, the sync seam and the manifest write).
///
/// Returns whether the machine accepted the event (`apply` succeeded); a rejected
/// event (e.g. a second `Failed` against an already-terminal machine) changes
/// nothing and notifies nothing — that terminal-state rejection IS the sink-side
/// half of the notify-once dedup.
///
/// Onset detection happens under the snapshot lock (compare-then-set): the
/// notification fires only when `error` / `warning` transitions `None → Some`.
/// A sticky warning that repeats (last-write-wins message update, Story 19.4)
/// updates the text but never re-fires. The `platform.notify` call itself runs
/// AFTER the lock is released — a slow/blocking notifier must never stall the
/// snapshot readers (tray tick, status poll, quit finalize).
fn fold_recording_event(
    machine: &mut RecordingSession,
    status: &Mutex<RecordingStatusVm>,
    platform: &dyn Platform,
    event: RecordingEvent,
) -> bool {
    let failure = match &event {
        RecordingEvent::Failed { message } => Some(message.clone()),
        _ => None,
    };
    // Story 19.4: a non-fatal warning (e.g. mic hot-unplug) is sticky on the
    // shared snapshot — last-write-wins message, never cleared back to `None`
    // mid-session, and NOT gated on `state == failed` (the session stays live;
    // the tray + banner render the warning beside the normal recording state).
    let warning = match &event {
        RecordingEvent::Warning { message, .. } => Some(message.clone()),
        _ => None,
    };
    let started = matches!(event, RecordingEvent::CaptureStarted);
    if machine.apply(event).is_err() {
        return false;
    }
    let (fault_onset, warning_onset) = {
        let mut snapshot = status_lock(status);
        snapshot.state = recording_ui_state(machine.state());
        snapshot.segments_closed = machine.segments_closed();
        if started && snapshot.started_at_epoch_ms.is_none() {
            snapshot.started_at_epoch_ms = Some(epoch_ms_now());
        }
        let fault_onset = failure.and_then(|message| {
            let onset = snapshot.error.is_none();
            snapshot.error = Some(message.clone());
            onset.then_some(message)
        });
        let warning_onset = warning.and_then(|message| {
            let onset = snapshot.warning.is_none();
            snapshot.warning = Some(message.clone());
            onset.then_some(message)
        });
        (fault_onset, warning_onset)
    };
    // The triad's native-notification leg (Story 18.4), outside the lock:
    // exactly once per fault onset, exactly once per warning onset. Bypasses
    // DND / per-Network mute inside the entry itself (see keeper-core::notify).
    if let Some(reason) = fault_onset {
        keeper_core::notify::notify_recording_fault(platform, &reason);
    }
    if let Some(message) = warning_onset {
        keeper_core::notify::notify_recording_warning(platform, &message);
    }
    true
}

/// Surface a `run_session` **task failure** (spawn fault, non-zero exit,
/// unsupported) that did not already arrive as a terminal sidecar event: flip the
/// snapshot to an honest `Failed` + `error` and fire the Story 18.4 fault
/// notification on onset — the [`recording_start`] driver's fallback leg.
///
/// Guarded on not-already-terminal: a session the sink already settled
/// (`Finalized`/`Recovered`/`Failed`) is left untouched, so a fault that surfaced
/// through the event path never double-notifies here (the sink set `error` under
/// this same mutex; the guard + the `None → Some` onset rule make the pair fire
/// exactly once between them). The notify call runs after the lock is released.
fn fail_recording_snapshot(
    status: &Mutex<RecordingStatusVm>,
    platform: &dyn Platform,
    message: String,
) {
    let fault_onset = {
        let mut snapshot = status_lock(status);
        if snapshot.state.is_live() || snapshot.state == RecordingUiState::Idle {
            snapshot.state = RecordingUiState::Failed;
            let onset = snapshot.error.is_none();
            snapshot.error = Some(message.clone());
            onset.then_some(message)
        } else {
            None
        }
    };
    if let Some(reason) = fault_onset {
        keeper_core::notify::notify_recording_fault(platform, &reason);
    }
}

/// How often the live disk-space guard probes the destination volume while a
/// recording session is live (Story 18.5): ~1 Hz — the same cadence as the tray
/// tick, fast enough that a warn/stop lands within about a second of the
/// threshold crossing, slow enough that the `statvfs` probe is free.
const DISK_GUARD_POLL: Duration = Duration::from_secs(1);

/// Execute one planned disk-guard action against the shared status snapshot
/// (Story 18.5): the shell-side executor for [`plan_disk_guard_action`]'s
/// verdict. `Warn`/`Stop` set the sticky `warning` (last-write-wins, the 19.4
/// model — the tray ⚠ line and banner amber render it on their next tick/poll)
/// under the snapshot lock, then post exactly one native notification through
/// the 18.4 warning entry (bypasses DND / per-Network mute) AFTER the lock is
/// released — a slow notifier must never stall the snapshot readers. `Stop`
/// additionally fires `request_stop` — in production the idempotent
/// [`stop_active_recording`] trigger, so the session runs the normal graceful
/// finalize path (`Stopping` → `Finalized`, never a `Failed` fault).
///
/// Notify-once is owned by the caller's [`DiskGuardLatch`]: each distinct
/// action arrives here at most once per session, and `None` (healthy band,
/// latched repeat, or failed probe read as plenty) is a strict no-op. The stop
/// trigger is a closure so this executor unit-tests without an `AppState`.
fn apply_disk_guard_action(
    status: &Mutex<RecordingStatusVm>,
    platform: &dyn Platform,
    action: DiskGuardAction,
    request_stop: impl FnOnce(),
) {
    let (message, stop) = match action {
        DiskGuardAction::None => return,
        DiskGuardAction::Warn { message } => (message, false),
        DiskGuardAction::Stop { message } => (message, true),
    };
    {
        let mut snapshot = status_lock(status);
        snapshot.warning = Some(message.clone());
    }
    // Distinct notification copy per leg: the warning entry asserts "the
    // recording is still running" (true for a warn, a lie for the stop), so the
    // hard-floor stop uses the dedicated `notify_recording_stopped` entry.
    if stop {
        keeper_core::notify::notify_recording_stopped(platform, &message);
        request_stop();
    } else {
        keeper_core::notify::notify_recording_warning(platform, &message);
    }
}

/// Acknowledge (dismiss) a settled recording session's outcome (Story 18.4): a
/// **terminal** slot (`finalized`/`recovered`/`failed`) is cleared back to the
/// idle snapshot — dropping `error`/`warning`, which releases the held error
/// tray/banner surfaces on the next tick/poll — while a **live** slot is left
/// strictly untouched (acknowledge must never be a silent stop). Returns whether
/// the slot was cleared. Shared by the [`recording_acknowledge`] command and the
/// tray's **Dismiss Error** item.
pub(crate) fn acknowledge_recording_slot(slot: &Mutex<Option<RecordingRun>>) -> bool {
    let mut guard = slot_lock(slot);
    let terminal = guard
        .as_ref()
        .is_some_and(|run| status_lock(&run.status).state.is_terminal());
    if terminal {
        // Dropping the run drops the spent stop trigger and (detached) driver
        // handle of a session that already reached its terminal state — the
        // next `recording_snapshot` read is the honest idle default.
        *guard = None;
    }
    terminal
}

/// Follow a retitled session's folder in the kept status snapshot (Story 40.4):
/// if the slot's `output_path` is exactly `from`, rewrite it to `to`.
///
/// **Why the shell has to do this.** The slot survives the session it describes
/// — `recording_stop` leaves the terminal snapshot in place so the summary card
/// can render it, and the frontend re-adopts `recording_status().output_path`
/// on every remount. A retitle moves that folder, so leaving the snapshot alone
/// hands the surface a path that no longer exists: the summary fetch fails
/// against it, Reveal-in-Finder opens nothing, and the card silently reverts to
/// the dead folder the next time the pane mounts.
///
/// Deliberately narrow. Only `output_path`, because that is the one field that
/// names a location; only on an EXACT match, because a slot describing a
/// different session must not be dragged along by a retitle of this one; and
/// never for a live session, which [`retitle_session_folder`] refuses before
/// anything moves (a live folder is in the reservation set). Returns whether the
/// snapshot was repointed, so a caller can log the miss rather than assume.
///
/// Takes the slot rather than the whole state ([`acknowledge_recording_slot`]'s
/// convention) so it is unit-testable without an `AppState`.
#[cfg(desktop)]
pub(crate) fn repoint_recording_slot_output(
    slot: &Mutex<Option<RecordingRun>>,
    from: &Path,
    to: &Path,
) -> bool {
    let guard = slot_lock(slot);
    let Some(run) = guard.as_ref() else {
        return false;
    };
    let mut snapshot = status_lock(&run.status);
    if snapshot.output_path.as_deref().map(Path::new) != Some(from) {
        return false;
    }
    snapshot.output_path = Some(to.to_string_lossy().into_owned());
    true
}

/// The current Unix epoch in milliseconds (0 on a pre-1970 clock — never a panic).
/// `pub(crate)` since Story 18.1: the tray's status line derives elapsed from the
/// same clock the driver task stamps `started_at_epoch_ms` with.
pub(crate) fn epoch_ms_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Enumerate the recordable sources — displays and applications — the source
/// picker polls (Story 19.1). Runs the sidecar `listSources` round-trip (a fresh
/// child `keeper-rec` per call; bounded by the shell's pre-flight timeout so a
/// wedged sidecar resolves a clean error, never a hung poll) and returns the live
/// [`RecordingSourcesVm`]: real displays plus real applications (name/pid/bundleId
/// + an optional ≤64px PNG icon data-URI, keeper excluded). Called on a ~3s poll
/// while the idle setup surface is visible and on window focus — but only once
/// Screen Recording is granted: the picker gates the poll on the pre-flight
/// verdict, and the sidecar independently skips the `SCShareableContent` leg
/// behind its non-prompting preflight, because that leg posts the OS permission
/// prompt. Two layers, one invariant: nothing but the explicit
/// `request_screen_recording_permission` may ever prompt. An empty `applications`
/// therefore means "not enumerated or none available", never "denied". Gated by
/// the `recording` capability — an unsupported platform answers `Unsupported`
/// with no spawn. Failures funnel through [`to_ipc_error`]; the picker swallows
/// them to the prior list (a transient enumeration failure never blanks the
/// picker).
#[tauri::command]
pub async fn recording_list_sources(
    state: State<'_, AppState>,
) -> Result<RecordingSourcesVm, IpcError> {
    if !crate::macos_version::recording_supported() {
        return Err(to_ipc_error(CoreError::Unsupported(
            "recording is not available on this platform".to_owned(),
        )));
    }
    state.recorder.list_sources().await.map_err(to_ipc_error)
}

/// Map a picker [`RecordingTargetVm`] into the session's manifest [`CaptureTarget`]
/// and the sidecar [`SessionParams`] video-target fields (Story 19.1). An
/// application target wins (records app-scoped; `display_id` unused); a display
/// target (or `None`) records the display (`None` = the main display, the
/// unchanged 16.6 path).
fn resolve_capture_target(
    target: Option<RecordingTargetVm>,
) -> (CaptureTarget, Option<u32>, Option<ApplicationTarget>, bool) {
    match target {
        Some(RecordingTargetVm::Application { pid, bundle_id }) => (
            CaptureTarget::application(bundle_id.clone(), pid),
            None,
            Some(ApplicationTarget { pid, bundle_id }),
            false,
        ),
        Some(RecordingTargetVm::Display { display_id }) => {
            (CaptureTarget::display(display_id), display_id, None, false)
        }
        // Story 21.3: no video target at all — the sidecar records
        // `audio-####.m4a` (system audio and/or mic) with no SCStream video.
        Some(RecordingTargetVm::AudioOnly) => (CaptureTarget::audio_only(), None, None, true),
        None => (CaptureTarget::display(None), None, None, false),
    }
}

/// How many collision ordinals one start will try before giving up.
///
/// The template decides where `{seq}` lands, so this is a bound on attempts, not
/// on a suffix: 64 sessions rendering to the same path inside the same minute is
/// not a collision any more, it is a loop somewhere, and failing with the path it
/// tried says more than spinning until the disk fills.
const SESSION_FOLDER_ATTEMPTS: u32 = 64;

/// The civil datetime + title a START renders against, at one ordinal.
///
/// Deliberately built from [`preview_render_ctx`]: the preview's promise is
/// "this is where a recording started now would land", and the only honest way
/// to keep that true is for both to be the same context with the same clock
/// read. Only `seq` differs — the preview shows ordinal 1 because that is the
/// common case, while a start walks upward as it discovers collisions.
///
/// Generic in the zone for [`preview_render_ctx`]'s reason: a start reads
/// `Local`, while Story 40.4's retitle renders a stamp's OWN offset.
fn start_render_ctx<Tz: TimeZone>(now: &DateTime<Tz>, title: Option<&str>, seq: u32) -> RenderCtx {
    RenderCtx {
        seq,
        ..preview_render_ctx(now, title)
    }
}

/// Join a rendered relative path onto the destination root.
///
/// Component by component, for the reason [`compose_path_preview`] does it that
/// way: a [`RelativePath`] is always `/`-separated, and pushing it whole would
/// leave those separators verbatim inside a Windows path.
fn session_folder_path(root: &Path, relative: &RelativePath) -> PathBuf {
    let mut folder = root.to_path_buf();
    for component in relative.components() {
        folder.push(component);
    }
    folder
}

/// What a start attempt created, so a failure can put the filesystem back.
///
/// A template may nest (`{yyyy}/…`), so a start can create directories that did
/// not exist before it — and a start that then fails must not leave them behind.
/// The unwind is deliberately narrow: deepest-first, `remove_dir` only (NEVER
/// `remove_dir_all` on a folder the user chose), and only the directories THIS
/// attempt created. A `2026/` that already held ten sessions is never a
/// candidate, and one that is not empty refuses to be removed by the syscall
/// itself rather than by a check that could race.
struct SessionScaffold {
    created: Vec<PathBuf>,
    committed: bool,
}

impl SessionScaffold {
    fn new() -> Self {
        Self {
            created: Vec::new(),
            committed: false,
        }
    }

    /// Record a directory this attempt brought into existence.
    fn created(&mut self, dir: PathBuf) {
        self.created.push(dir);
    }

    /// The session exists and is the caller's problem now — keep everything.
    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for SessionScaffold {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for dir in self.created.iter().rev() {
            // Errors are the expected case for a directory another session
            // populated meanwhile, and there is nothing a failing start can do
            // about it beyond not making it worse.
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Bring a rendered path's PARENT directories into existence, registering only
/// the ones THIS attempt created with `scaffold`.
///
/// A nesting template's intermediate directories are created one at a time, so
/// the unwind knows exactly which ones this attempt brought into existence.
/// `create_dir` per component rather than `create_dir_all`: the latter cannot
/// tell "I made this" from "it was already here", and that difference is the
/// only thing the unwind may act on.
///
/// Shared by the start and Story 40.4's retitle, so a nesting template's
/// intermediates are created — and unwound — identically whichever one puts a
/// session there.
fn create_session_intermediates(
    root: &Path,
    relative: &RelativePath,
    scaffold: &mut SessionScaffold,
) -> Result<(), IpcError> {
    let mut parent = root.to_path_buf();
    let mut intermediates = relative.components().collect::<Vec<_>>();
    intermediates.pop();
    for component in intermediates {
        parent.push(component);
        match std::fs::create_dir(&parent) {
            Ok(()) => scaffold.created(parent.clone()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(session_path_error(relative, &err)),
        }
    }
    Ok(())
}

/// The refusal for a rendered path the filesystem would not take.
///
/// Names the rendered RELATIVE path, which is the half the user authored and can
/// fix; the absolute root is already on screen in the same card. Built here
/// rather than through [`RecordingError::ManifestIo`] because that type's
/// contract is to carry no path at all (`manifest_io`), and a pre-flight failure
/// that does not say which path it tried is the kind of error people file a
/// ticket about. Retriable: a full volume, a locked folder and a disconnected
/// disk all clear without the user changing a thing.
fn session_path_error(relative: &RelativePath, cause: &std::io::Error) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "the session folder \"{}\" could not be created under the destination: {cause}",
            relative.as_str()
        ),
        account_id: None,
        retriable: true,
    }
}

/// This device's stable ULID, read the way sync reads it — once per process.
///
/// `sync.db`'s device row is the identity of the DEVICE, not of sync: it is what
/// every commit's `Keeper-Device` trailer publishes, and minting a second one for
/// recordings would be a second answer to "which machine made this". Read
/// through `keeper_sync::db` directly rather than through `crate::sync::engine`,
/// because engine construction legitimately fails without git and a machine with
/// no git must still be able to record.
///
/// Cached, because the identity cannot change while the process lives and the
/// uncached read is three bad things on the Record click: it opens and MIGRATES
/// `sync.db` (which the running engine and `keeper-syncd` also write, with no
/// busy timeout on this connection, so a sync tick could refuse a recording),
/// and it forks `hostname` for a label `device_identity` only uses on the very
/// first call this device ever makes.
#[cfg(desktop)]
fn device_ulid(data_dir: &Path) -> Result<String, IpcError> {
    static DEVICE_ULID: OnceLock<String> = OnceLock::new();
    if let Some(cached) = DEVICE_ULID.get() {
        return Ok(cached.clone());
    }
    let conn = keeper_sync::db::open(data_dir).map_err(|err| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("the device identity could not be opened: {err}"),
        account_id: None,
        retriable: true,
    })?;
    // The label seeds the row only when this device has never had one, so it is
    // computed here rather than above the cache check.
    let identity = keeper_sync::db::device_identity(&conn, &crate::sync::read_host_label())
        .map_err(|err| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("the device identity could not be read: {err}"),
            account_id: None,
            retriable: true,
        })?;
    Ok(DEVICE_ULID.get_or_init(|| identity.id).clone())
}

/// The mobile twin: there is no `sync.db` to read.
///
/// Folder sync is a desktop-only dependency, so `keeper_sync` is not even linked
/// here — and neither is a recorder (`IosRecorder::is_available` is `false` and
/// its `run_session` returns `Unsupported`). The refusal quotes that recorder's
/// own sentence so the two paths are indistinguishable to a caller, and it now
/// refuses before a session folder is created rather than after.
#[cfg(not(desktop))]
fn device_ulid(_data_dir: &Path) -> Result<String, IpcError> {
    Err(to_ipc_error(CoreError::Unsupported(
        if cfg!(target_os = "ios") {
            "recording is not available on iOS".to_owned()
        } else {
            "recording is not available on this platform".to_owned()
        },
    )))
}

/// Create the session folder for one start, walking the template's ordinal.
///
/// The retry IS the template's `{seq}` (FR-128): the ordinal lands wherever the
/// template put it instead of always at the end of the name, and the leaf's
/// `create_dir` — not a prior `exists()` check — is what decides, so two starts
/// racing inside the same minute cannot both win the same folder.
///
/// `make` is what turns a chosen folder into a manifest. The real caller passes
/// [`SessionManifest::create_with_meta`]; taking it as a closure is what makes
/// the retry, the unwind and the exhaustion refusal testable without an
/// `AppState`, a sidecar or a clock.
///
/// Returns the manifest, its absolute folder, and the live-folder reservation —
/// which is taken BEFORE each candidate is created (Story 17.3) so the
/// orphan-recovery pass can never rewrite a manifest this start is about to own,
/// and dropped again when an ordinal turns out to be taken.
fn create_session_folder<F>(
    reserved: &Arc<Mutex<HashSet<PathBuf>>>,
    root: &Path,
    template: &PathTemplate,
    now: &DateTime<Local>,
    title: Option<&str>,
    mut make: F,
) -> Result<(SessionManifest, PathBuf, LiveFolderReservation), IpcError>
where
    F: FnMut(PathBuf) -> Result<SessionManifest, RecordingError>,
{
    let mut last_relative: Option<RelativePath> = None;
    for seq in 1..=SESSION_FOLDER_ATTEMPTS {
        let relative = template.render(&start_render_ctx(now, title, seq));
        let folder = session_folder_path(root, &relative);
        let mut scaffold = SessionScaffold::new();
        create_session_intermediates(root, &relative, &mut scaffold)?;
        let reservation = LiveFolderReservation::reserve(reserved, folder.clone());
        match make(folder.clone()) {
            Ok(manifest) => {
                scaffold.commit();
                return Ok((manifest, folder, reservation));
            }
            // That ordinal is taken. Drop the reservation, let the scaffold
            // remove only the directories this attempt created, and render the
            // next one.
            Err(RecordingError::SessionFolderExists) => {
                drop(reservation);
                last_relative = Some(relative);
            }
            Err(err) => {
                // The leaf may exist with nothing usable in it; the scaffold
                // takes it out along with the parents this attempt made, so a
                // failed start leaves the destination as it found it.
                scaffold.created(folder);
                return Err(to_ipc_error(err.into()));
            }
        }
    }
    // Every ordinal was taken. Name the last path tried: with 64 siblings in
    // place the template itself is the problem, and the user can only see that
    // if the refusal says what it rendered.
    let relative = last_relative
        .unwrap_or_else(|| template.render(&start_render_ctx(now, title, SESSION_FOLDER_ATTEMPTS)));
    Err(IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "the session folder \"{}\" already exists, and so did every alternative up to {}",
            relative.as_str(),
            SESSION_FOLDER_ATTEMPTS
        ),
        account_id: None,
        retriable: false,
    })
}

/// Mint a session's immutable identity: `<device ULID>-<session ULID>`.
///
/// Device-scoped by AD-73, so two machines recording into one synced folder in
/// the same minute cannot collide however identical their rendered paths are.
/// One scalar, because story 42 makes it a primary key; split on the single `-`
/// to recover the device half, which Crockford's alphabet guarantees is
/// unambiguous (it has no `-`); safe inside a markdown link and a shell word,
/// because story 42 offers it as copyable text and as a `session:` link target.
fn mint_session_id(data_dir: &Path) -> Result<String, IpcError> {
    Ok(format!("{}-{}", device_ulid(data_dir)?, ulid::Ulid::new()))
}

/// The typed refusal for a retitle of a session whose folder is claimed by
/// something else (Story 40.4).
///
/// **Why one message for two states.** The reservation set holds paths, not
/// reasons, so a claim it refuses is either a live recording or another retitle
/// of the same session already in flight — and the shell cannot tell which
/// without a second registry that would have to be kept in step with this one.
/// Rather than mint a second wire code for a state the surface would treat
/// identically, the sentence is worded to be TRUE in both: "stop the recording
/// first" is a lie to the second retitler, and there is nothing for them to
/// stop.
///
/// Typed rather than `Internal`, because the surface has something useful to say
/// and "internal error" would send the user looking for a fault instead. Not
/// retriable: while a recording runs nothing clears, and the driver and the
/// sidecar hold absolute paths into that folder, so moving it underneath them
/// would break the very session the user is recording.
#[cfg(desktop)]
fn recording_session_live_error() -> IpcError {
    IpcError {
        code: IpcErrorCode::RecordingSessionLive,
        message: "this session's folder is busy — it is still recording, or it is already being renamed; finish that before renaming it".to_owned(),
        account_id: None,
        retriable: false,
    }
}

/// The refusal for a retitle whose manifest rewrite failed AND whose folder
/// could not be moved back (Story 40.4).
///
/// Distinct from the manifest-write error it replaces because the two describe
/// different worlds. "The manifest could not be written" implies the session is
/// still where the caller left it; on this path it is not — the rename landed,
/// the rewrite did not, and the move back failed too. The card repaints from the
/// path it asked about, so an error that does not name the new one leaves it
/// painting a folder that no longer exists and a Reveal that cannot resolve.
/// Names the ABSOLUTE location, because that is the only thing the user can act
/// on. Retriable: the folder is intact at the new path, and a retitle aimed
/// there rewrites the manifest as soon as whatever blocked the write clears.
#[cfg(desktop)]
fn retitle_stranded_error(destination: &Path, cause: &IpcError) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "the session folder was moved to \"{}\", but its manifest could not be rewritten there and it could not be moved back: {}",
            destination.display(),
            cause.message
        ),
        account_id: None,
        retriable: true,
    }
}

/// The instant a retitle re-renders against: the session's OWN start, in the
/// offset that start was written in.
///
/// Never the clock. A session recorded last Tuesday must not migrate into this
/// week's folder because it was renamed today, so the only honest input is the
/// `started_at` stamp `recording_start` wrote (RFC 3339 with its offset, Story
/// 21.5) — and it is returned as the `FixedOffset` it carries, NOT converted to
/// the machine's current zone. The offset in the stamp is the one the machine
/// was in when it named the folder, so reading the six civil fields off it
/// reproduces exactly the numbers the start rendered from. Converting to `Local`
/// first would re-render a session stamped `2026-01-01T00:30:00+02:00` as
/// `2025-12-31 2230` on a machine that has since moved zones — a different YEAR
/// folder, and a retitle that no longer agrees with a fresh start about where
/// the session belongs.
///
/// A manifest that predates that stamp has no start at all. The folder's own
/// modification time is the closest honest answer, and taking it is LOGGED: a
/// session that lands in a surprising month must have a reason on record, and a
/// silent fall back to `now` would be exactly the migration this function
/// exists to prevent. There is no stored offset for that case, so the local one
/// is the only reading available.
#[cfg(desktop)]
fn session_start_instant(
    manifest: &SessionManifest,
    folder: &Path,
) -> Result<DateTime<FixedOffset>, IpcError> {
    match manifest.started_at.as_deref() {
        Some(stamp) => match DateTime::parse_from_rfc3339(stamp) {
            Ok(started_at) => return Ok(started_at),
            // A stamp that does not parse is a corrupt manifest, not an old one,
            // so it is `warn` where an absent stamp is `debug`.
            Err(error) => tracing::warn!(
                %error,
                "retitle: the manifest's start stamp does not parse; falling back to the folder's modification time"
            ),
        },
        None => tracing::debug!(
            "retitle: the manifest carries no start stamp; falling back to the folder's modification time"
        ),
    }
    let modified = folder
        .metadata()
        .and_then(|metadata| metadata.modified())
        .map_err(|error| IpcError {
            code: IpcErrorCode::Internal,
            message: format!("the session's start time could not be determined: {error}"),
            account_id: None,
            retriable: true,
        })?;
    Ok(DateTime::<Local>::from(modified).fixed_offset())
}

/// Point a manifest at the folder it now lives in, with its new title.
///
/// The order is load-bearing: [`SessionManifest::write`] targets the manifest's
/// own folder, so the rebind has to happen before the write or the retitled
/// manifest lands back in the folder the session just left. `retitle` trims and
/// treats blank as cleared, which is the same rule [`preview_render_ctx`]
/// applies to the title it renders from — so the stored title and the folder
/// that title named can never disagree.
#[cfg(desktop)]
fn rewrite_retitled_manifest(
    manifest: &mut SessionManifest,
    destination: &Path,
    title: Option<&str>,
) -> Result<(), IpcError> {
    manifest.retitle(title.map(str::to_owned));
    manifest.rebind_folder(destination.to_path_buf());
    manifest.write().map_err(|err| to_ipc_error(err.into()))
}

/// Whether two paths name the SAME directory on disk, not merely the same
/// bytes.
///
/// Story 40.4's in-place branch turns on "the render landed where the session
/// already is", and a byte comparison answers that question wrong on a
/// case-insensitive volume: `…1432 Standup` and `…1432 standup` are unequal
/// `PathBuf`s and the SAME directory on APFS, so a case-only retitle would skip
/// the in-place branch, collide with itself on `create_dir` and take a
/// permanent ` (2)` suffix. `canonicalize` resolves each component through the
/// filesystem, which is what corrects the case (and any symlink) on the
/// platforms that fold it. A destination that does not exist yet cannot
/// canonicalize — the normal case — and is correctly not the source.
#[cfg(desktop)]
fn is_same_directory(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Move a finished session to where the CURRENT template renders its new title,
/// and leave its identity where it is (Story 40.4).
///
/// The whole retitle decision, with no `AppState`, no sidecar and no clock in
/// it, so every row of the story's matrix is a test over a temp root.
///
/// The naming is [`create_session_folder`]'s, deliberately: the same effective
/// template, the same [`start_render_ctx`], the same ordinal walk and the same
/// [`SessionScaffold`] unwind. Only the instant (the session's own start, not
/// `now`) and the title differ, which is what makes a retitle and a fresh start
/// of the same session agree on where it belongs.
///
/// Returns the rewritten manifest and its new absolute folder — the same folder
/// when the render lands on the one the session already occupies.
#[cfg(desktop)]
fn retitle_session_folder(
    reserved: &Arc<Mutex<HashSet<PathBuf>>>,
    root: &Path,
    template: &PathTemplate,
    folder: &Path,
    title: Option<&str>,
) -> Result<(SessionManifest, PathBuf), IpcError> {
    // Claim the folder in the live set, and let the CLAIM be the live check:
    // `reserve` reports whether THIS guard inserted the entry, so a folder a
    // live (or starting) session — or another retitle of the same session —
    // already holds is refused as one indivisible compare-and-set instead of a
    // `contains` that a start could win the instant after it read `false`. A
    // claim is held for the whole move (repointed onto the destination at the
    // rename, never released across it), which also keeps the orphan-recovery
    // pass from reconciling and rewriting this manifest from under the rename.
    let mut claim = LiveFolderReservation::reserve(reserved, folder.to_path_buf());
    if !claim.owned {
        return Err(recording_session_live_error());
    }
    // A folder whose manifest does not load is not a session, and a retitle is
    // the wrong tool for whatever it is: the manifest is both the thing being
    // rewritten and the only place the start instant and the identity live.
    let mut manifest = SessionManifest::load(folder).map_err(|err| to_ipc_error(err.into()))?;
    let started_at = session_start_instant(&manifest, folder)?;
    let mut last_relative: Option<RelativePath> = None;
    for seq in 1..=SESSION_FOLDER_ATTEMPTS {
        let relative = template.render(&start_render_ctx(&started_at, title, seq));
        let destination = session_folder_path(root, &relative);
        // The render landed on the folder the session already occupies — a
        // template with no title in it, a title that slugs to the same thing,
        // the same title again, or a case-only change on a volume that folds
        // case. There is nothing to move, and `create_dir` here would only
        // report a collision against the session itself. The manifest is
        // rebound to `folder`, not to the render: on a folding volume the two
        // differ in case and only `folder` is the name the directory actually
        // has, so the `session` label cannot drift away from the disk.
        if destination == folder || is_same_directory(&destination, folder) {
            rewrite_retitled_manifest(&mut manifest, folder, title)?;
            return Ok((manifest, folder.to_path_buf()));
        }
        let mut scaffold = SessionScaffold::new();
        create_session_intermediates(root, &relative, &mut scaffold)?;
        match std::fs::create_dir(&destination) {
            // The leaf is registered with the scaffold even though the move is
            // about to fill it: `remove_dir` refuses a non-empty directory, so an
            // unwind can never take the session with it, and every failure below
            // therefore leaves the destination exactly as this attempt found it.
            Ok(()) => scaffold.created(destination.clone()),
            // That ordinal is taken. `create_dir` — never a prior `exists()` —
            // is what decided, so two retitles racing for one name cannot both
            // win it; the loser walks to the next ordinal.
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                last_relative = Some(relative);
                continue;
            }
            Err(err) => return Err(session_path_error(&relative, &err)),
        }
        // The `create_dir` above is the arbiter — it is what serialises two
        // retitles racing for one name, and it is a syscall rather than a check
        // so neither can win it twice. `remove_dir` here is a separate concern:
        // POSIX `rename(2)` replaces an empty directory, but Windows
        // `MoveFileExW` refuses `MOVEFILE_REPLACE_EXISTING` for a directory at
        // all, so the target has to NOT exist when the rename runs. Taking the
        // claim first and dropping it only once this attempt is committed to the
        // rename on the next line is what keeps the arbitration while making the
        // move portable.
        if let Err(err) = std::fs::remove_dir(&destination) {
            return Err(session_path_error(&relative, &err));
        }
        // The rename is what keeps the media untouched: one directory entry
        // moves, no byte of the session is read or copied.
        if let Err(err) = std::fs::rename(folder, &destination) {
            // The leaf is gone again, so the scaffold's `remove_dir` would only
            // fail on it; the intermediates this attempt made still unwind.
            return Err(session_path_error(&relative, &err));
        }
        // The source is vacated. A claim left pointing at it would un-reserve a
        // start that reoccupies the name the moment this guard drops, so the
        // claim moves onto the destination as one locked step — the session is
        // reserved on both sides of the rename and unreserved on neither.
        claim.repoint(destination.clone());
        if let Err(error) = rewrite_retitled_manifest(&mut manifest, &destination, title) {
            // The session is at the new path with its old manifest inside it. Put
            // it back, so a refused retitle is a retitle that did not happen
            // rather than a folder whose name and manifest disagree. The scaffold
            // then takes out the (now empty again) leaf and the intermediates.
            match std::fs::rename(&destination, folder) {
                Ok(()) => claim.repoint(folder.to_path_buf()),
                // The move happened and cannot be undone: the session is
                // somewhere the caller was never told about. Saying "the
                // manifest could not be written" here would be a true sentence
                // about a false world — the card would keep painting the old
                // folder, which no longer exists.
                Err(unwind) => {
                    tracing::error!(
                        %unwind,
                        destination = %destination.display(),
                        "retitle: the manifest write failed and the folder could not be moved back"
                    );
                    scaffold.commit();
                    return Err(retitle_stranded_error(&destination, &error));
                }
            }
            return Err(error);
        }
        scaffold.commit();
        return Ok((manifest, destination));
    }
    // Every ordinal was taken. Name the last path tried, exactly as a start
    // does: with 64 siblings in place the template is the problem, and the user
    // can only see that if the refusal says what it rendered.
    let relative = last_relative.unwrap_or_else(|| {
        template.render(&start_render_ctx(
            &started_at,
            title,
            SESSION_FOLDER_ATTEMPTS,
        ))
    });
    Err(IpcError {
        code: IpcErrorCode::Internal,
        message: format!(
            "the session folder \"{}\" already exists, and so did every alternative up to {}",
            relative.as_str(),
            SESSION_FOLDER_ATTEMPTS
        ),
        account_id: None,
        retriable: false,
    })
}

/// When a committed segment is PUBLISHED (Story 41.5, FR-136, AD-70) — this
/// session's copy of the destination profile's `PushPolicy`, read ONCE at start.
///
/// Platform-free for [`DestinationProfileRow`]'s reason: `keeper-sync`'s
/// `PushPolicy` exists only on desktop, and what a driver sink needs of it is one
/// question — is a segment that just closed worth asking about now?
///
/// A session-captured COPY rather than a per-segment read, because a policy the
/// sink re-read would be a policy that can change mid-session: an edit made while
/// a meeting is being recorded would start pushing gigabyte objects over the
/// uplink that meeting runs on. The policy in force is the one that was in force
/// when Record was pressed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SessionPushPolicy {
    /// `PushPolicy::Immediate` — every committed segment is worth publishing now.
    PerSegment,
    /// `PushPolicy::SessionEnd` — the profile default, and the reason it is the
    /// `Default` here too: it publishes nothing while the recorder runs, so every
    /// degrade path (no engine, unreadable profile, no recordings block) lands on
    /// the quiet answer rather than on someone's uplink.
    #[default]
    AtSessionEnd,
    /// `PushPolicy::Window { .. }` — the engine owns the clock, so the sink asks
    /// on every segment and whether this instant is inside the quiet hours is the
    /// engine's answer, never a second implementation of the window here.
    InQuietHours,
}

impl SessionPushPolicy {
    /// Whether a segment that just committed is worth asking the engine about.
    ///
    /// `SessionEnd` is the only policy that answers no, and it answers no HERE
    /// rather than inside the engine: an ask IS a policy read, and a policy read
    /// per rotation is the surprise this story exists to prevent.
    fn asks_at_segment(self) -> bool {
        !matches!(self, Self::AtSessionEnd)
    }
}

/// What is asking a profile to push (Story 41.5): one segment's commit, or the
/// end of the session. Carried rather than inferred, so the engine's decision and
/// its log line can both name the cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingPushTrigger {
    /// A segment closed and its commit is due.
    SegmentCommitted,
    /// The session finalized — `manifest.json` is written and the folder is done.
    SessionEnd,
}

/// What the engine knows LOCALLY about one path's durability (Story 41.6,
/// FR-138) — the four facts, and nothing derived.
///
/// A shell-local mirror of `keeper_sync::engine::PathDurability` for
/// [`SessionPushPolicy`]'s reason: the type has to exist on a platform
/// `keeper-sync` is not linked into, so the port stays platform-free and the
/// mapping into a [`RecordingDurabilityVm`] is one total function that compiles
/// and is tested on a machine with no `git` at all.
///
/// The three booleans are independent readings rather than one enum because
/// that is how the engine holds them; collapsing them is the DERIVATION
/// ([`durability_state`]), and doing it in exactly one place is what keeps the
/// banner and the tray from ranking them differently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SegmentDurability {
    /// The path is in a commit in this profile.
    committed: bool,
    /// That commit is on the remote.
    pushed: bool,
    /// The engine has verified the pushed objects.
    verified: bool,
    /// Why publication has not happened, in the engine's own words — a push the
    /// remote refused, an unreachable remote. `None` when nothing is wrong.
    problem: Option<String>,
}

/// Everything a recording session asks of the sync engine (Stories 41.5 +
/// 41.6), and nothing else.
///
/// A trait rather than an `Arc<Engine>` for two reasons. The engine is
/// desktop-only while this sink is not, so a recorder holding one would not
/// compile on iOS; and an `Engine` can commit, push, pause and delete profiles,
/// none of which a recorder may do (AD-68) — the inbound direction 41.4
/// established is the whole point. Five questions is the whole surface: four a
/// session ASKS while it records, and one — [`Self::path_durability`] — the
/// status poll asks ABOUT what it recorded. The COUNTS 41.5's acceptance
/// criteria are stated in are all countable through it, and so is 41.6's whole
/// I/O matrix.
///
/// **Every method is total.** A refusal is a value, never a `?`: capture never
/// degrades (NFR-34), so there is nothing here for a sink to propagate, nothing
/// that can fail a command, and nothing that can stop a recorder. The one
/// `Result` is [`Self::path_durability`]'s, and its caller swallows it too — it
/// exists so the degrade has a sentence to log.
trait RecordingSyncPort: Send + Sync {
    /// This profile's push policy, read once per session by
    /// [`begin_recording_sync`]. Unreadable ⇒ [`SessionPushPolicy::AtSessionEnd`].
    fn push_policy(&self, profile_id: &str) -> SessionPushPolicy;

    /// Ensure this profile's `.gitattributes` carries the LFS rule for
    /// `extension`, writing it at most once (FR-137). `Ok(true)` means this call
    /// wrote it.
    ///
    /// The error is a sentence for a log line rather than a `SyncError`, for
    /// [`DestinationProfileTable`]'s reason: the type has to exist on a platform
    /// `keeper-sync` is not linked into.
    fn ensure_lfs_rule(&self, profile_id: &str, extension: &str) -> Result<bool, String>;

    /// Assert that `path` — a segment that just closed — will never be written
    /// again (Story 41.4, FR-134). `false` means the assertion was dropped and
    /// the file takes the ordinary settle window, which is exactly what would
    /// have happened if this seam did not exist.
    fn note_finished(&self, profile_id: &str, path: &Path) -> bool;

    /// Ask this profile to push if its own policy says now.
    ///
    /// Returns nothing because it cannot honestly return anything: publishing an
    /// LFS object that may be gigabytes is a network operation, and it must not
    /// run on the driver task. The implementation hands it to the runtime and the
    /// outcome is logged where it lands (NFR-32).
    fn request_push(&self, profile_id: &str, trigger: RecordingPushTrigger);

    /// What the engine knows LOCALLY about `path` right now (Story 41.6,
    /// FR-138) — is it committed, is that commit pushed, has it been verified,
    /// and is there a recorded reason it has not been published.
    ///
    /// **Cheap and local-only, because this is on the ~1 Hz poll path.** It may
    /// never do a network round trip: if the honest answer needs the wire, the
    /// answer is the last thing keeper knows locally. `Err` is a transient read
    /// failure — a sentence for one log line — and the caller degrades to the
    /// last known state rather than propagating it, because a status poll must
    /// not be able to fail.
    fn path_durability(&self, profile_id: &str, path: &Path) -> Result<SegmentDurability, String>;
}

/// The sync half of one recording session, resolved ONCE at start and carried
/// into the driver task (Story 41.5).
///
/// `None` on a [`RecordingSink`] is a plain-folder destination, and it says so
/// structurally: there is no profile to assert to, no policy to obey, and no
/// engine call to make.
struct RecordingSyncSession {
    /// The destination profile's id — every call names it.
    profile_id: String,
    /// The profile's resolved recordings root. Carried so a path that cannot
    /// possibly be inside it is never asserted: the engine refuses such an
    /// assertion by contract (Story 41.4), and 48 refusals would be 48 warn lines
    /// about a fact that was already known before the first rotation.
    recordings_root: PathBuf,
    /// The push policy in force for this session.
    push: SessionPushPolicy,
    /// The engine seam itself.
    port: Arc<dyn RecordingSyncPort>,
}

/// Open the sync half of a session (Story 41.5): resolve the destination profile
/// once, read the push policy in force, and write the session's LFS rule before a
/// recorder exists.
///
/// **Why the rule is written here.** `.gitattributes` is a tracked file, so
/// writing it IS a working-tree change; writing it on the first commit would
/// change the tree under a running recorder, and a rotation is the worst moment
/// to do that (FR-137). Written before the sidecar spawns, it is a change that
/// happened while nothing was recording.
///
/// A plain-folder destination returns `None` without touching the port at all —
/// there is nothing to ask a profile that is not there.
fn begin_recording_sync(
    destination: &RecordingDestination,
    media_extension: &str,
    port: Option<Arc<dyn RecordingSyncPort>>,
) -> Option<RecordingSyncSession> {
    let profile_id = destination.profile_id.clone()?;
    // No engine means no usable `git` (AD-41), which is a degrade this epic is
    // built to survive: the session records to disk and commits nothing.
    let port = port?;
    let push = port.push_policy(&profile_id);
    match port.ensure_lfs_rule(&profile_id, media_extension) {
        Ok(true) => tracing::info!(
            profile = %profile_id,
            extension = media_extension,
            "recordings sync: this session's media LFS rule was written before capture started"
        ),
        Ok(false) => tracing::debug!(
            profile = %profile_id,
            extension = media_extension,
            "recordings sync: this session's media LFS rule is already in place"
        ),
        // A missing rule costs LFS on this session's segments, not the session:
        // they commit as ordinary blobs and the recorder never learns.
        Err(reason) => tracing::warn!(
            %reason,
            profile = %profile_id,
            extension = media_extension,
            "recordings sync: the LFS rule could not be written; this session's segments commit without it"
        ),
    }
    Some(RecordingSyncSession {
        profile_id,
        recordings_root: destination.root.clone(),
        push,
        port,
    })
}

/// This session's seed segment name and the media extension whose LFS rule
/// covers it (Story 21.3 + 41.5, FR-137).
///
/// One function, because the two must agree: the sidecar numbers its rotations
/// from the name it is seeded with, so the extension every segment of this
/// session carries is that name's — and a rule written for any other extension
/// would cover nothing that exists.
fn session_media_seed(audio_only: bool) -> (&'static str, &'static str) {
    if audio_only {
        ("audio-0000.m4a", "m4a")
    } else {
        ("screen-0000.mov", "mov")
    }
}

/// The ledger line one sidecar event produces, if it produces one (Story 17.2).
///
/// Read BEFORE the event is applied, because `apply` consumes it. The basename
/// comes from the sidecar-reported path (synthesized from the track and the index
/// when absent — a `track:"camera"` line without a path must never fabricate a
/// `screen-####` name, Story 20.1); `bytes`/`track` degrade to 0/`"screen"`. This
/// is only the LIVE view — the terminal reconcile rebuilds the list
/// authoritatively from disk.
fn segment_ledger_entry(event: &RecordingEvent) -> Option<SegmentEntry> {
    let RecordingEvent::SegmentClosed {
        index,
        path,
        bytes,
        track,
        pts_start,
        pts_end,
    } = event
    else {
        return None;
    };
    Some(SegmentEntry {
        index: *index,
        file: path
            .as_deref()
            .and_then(|p| Path::new(p).file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                let stem = if track.as_deref() == Some("camera") {
                    "camera"
                } else {
                    "screen"
                };
                format!("{stem}-{index:04}.mp4")
            }),
        bytes: bytes.unwrap_or(0),
        track: track.clone().unwrap_or_else(|| "screen".to_owned()),
        // Story 17.4 (NFR-22): the host-clock PTS bounds exist only in this
        // event — the terminal disk reconcile preserves them by index (they
        // cannot be re-read from the rebased segment files).
        pts_start: *pts_start,
        pts_end: *pts_end,
    })
}

/// Everything one sidecar event does (Story 17.2 + 18.4 + 41.5): the snapshot
/// fold, the segment ledger, the finished-path assertion, and — at a terminal —
/// the session's single manifest write and its push.
///
/// A named type rather than the closure it used to be, because this story's
/// acceptance criteria are stated in COUNTS — 48 ledger lines, ONE
/// `.gitattributes` write, ONE `manifest.json` write, one push — and a closure
/// built inside [`recording_start`] is reachable only through a Tauri `State` and
/// a real `keeper-rec` child. Driving this directly with synthetic events is also
/// the only way to run a four-hour session in a millisecond.
struct RecordingSink {
    /// The platform-free session machine every event folds through.
    machine: RecordingSession,
    /// This session's manifest — written ONCE, at [`Self::finalize`].
    manifest: SessionManifest,
    /// The snapshot the status poll, the tray tick and the disk guard read.
    status: Arc<Mutex<RecordingStatusVm>>,
    /// The notification port for the Story 18.4 fault/warning triad.
    platform: Arc<dyn Platform>,
    /// The destination profile this session commits into (Story 41.5), or `None`
    /// for a plain folder.
    sync: Option<RecordingSyncSession>,
    /// The archive half of this session (Story 42.1), or `None` when the app has
    /// no `archive.db` open. Shared with the status path's durability reader —
    /// same session, same row, one place that knows how to describe it.
    archive: Option<Arc<RecordingArchiveSession>>,
}

impl RecordingSink {
    /// Fold one sidecar event.
    ///
    /// A rejected event (the machine's `apply` said no — a second `Failed`
    /// against an already-terminal session) does nothing at all: the ledger, the
    /// assertion and the manifest are consequences of a transition that did not
    /// happen.
    fn handle(&mut self, event: RecordingEvent) {
        // Story 22.5: while debug mode is on, every sidecar event lands as one
        // timestamped line in the session's `events.log` (beside
        // `manifest.json`) — the raw stream a bug report needs. Gated and
        // best-effort inside the helper; zero cost while off.
        if crate::debug_log::enabled() {
            crate::debug_log::session_event(self.manifest.folder(), &format!("{event:?}"));
        }
        let segment = segment_ledger_entry(&event);
        // Machine + snapshot fold, plus the Story 18.4 onset-deduped fault/
        // warning notification (see `fold_recording_event`).
        if !fold_recording_event(
            &mut self.machine,
            &self.status,
            self.platform.as_ref(),
            event,
        ) {
            return;
        }
        if let Some(entry) = segment {
            // The absolute path is built from the ledger entry, before it moves
            // into the ledger: what is asserted is then exactly the line that was
            // recorded, never a second guess at the name. The segment lives in
            // the session folder by construction — the sidecar writes nowhere
            // else — so joining the folder is a fact, not a search.
            let path = self.manifest.folder().join(&entry.file);
            // Story 42.1: the index learns the same closed segment the ledger is
            // about to record and 41.5 is about to assert — one row per (session,
            // index, track), path RELATIVE to the destination root. Sent from
            // `&entry` before the move so the row and the ledger line describe
            // the same bytes with no second guess and no clone. Best-effort like
            // both of its neighbours: a message on a channel, never a rotation
            // the recorder can notice.
            if let Some(archive) = self.archive.as_ref() {
                // The host clock, read here rather than passed in, because a
                // close time is a fact about when THIS host saw the rotation —
                // the PTS bounds beside it are the capture clock's answer.
                archive.segment(
                    self.manifest.folder(),
                    &entry,
                    Local::now().timestamp_millis(),
                );
            }
            self.manifest.record_segment(entry);
            self.commit_finished_segment(&path);
        }
        if matches!(
            self.machine.state(),
            SessionState::Finalized | SessionState::Recovered | SessionState::Failed
        ) {
            self.finalize();
        }
    }

    /// Hand one closed segment to the destination profile (Story 41.5, FR-136):
    /// assert that it is finished, then ask for a push if this session's policy
    /// publishes per segment.
    ///
    /// **Never slow, never fallible.** The assertion is a `try_send` into a
    /// bounded queue and the push is handed to the runtime, because this runs on
    /// the driver task that also folds every event into the live snapshot: a sink
    /// that waited on the network would stall the status the tray, the banner and
    /// the disk guard read. Committing is the engine's next pass, not this call.
    ///
    /// A plain-folder destination reaches none of it.
    fn commit_finished_segment(&self, path: &Path) {
        let Some(sync) = self.sync.as_ref() else {
            return;
        };
        // The engine refuses an assertion outside the profile's recordings root
        // by contract (Story 41.4). Refusing it here too keeps a misplaced
        // session from spending a warn line per rotation on a fact that was true
        // before the first one.
        if !path.starts_with(&sync.recordings_root) {
            tracing::debug!(
                profile = %sync.profile_id,
                "recordings sync: this session is not inside the profile's recordings root, so its segments take the ordinary settle window"
            );
            return;
        }
        sync.port.note_finished(&sync.profile_id, path);
        if sync.push.asks_at_segment() {
            sync.port
                .request_push(&sync.profile_id, RecordingPushTrigger::SegmentCommitted);
        }
    }

    /// The end of the session: the ONE `manifest.json` write, then the policy's
    /// push (Story 41.5, FR-146).
    ///
    /// **Why the manifest is written here and nowhere else.** It used to be
    /// rewritten on every event, so a four-hour session rewrote its metadata 48
    /// times under a recorder that was still running — and for a synced
    /// destination that is 48 working-tree changes, each one a commit saying
    /// nothing new about a segment that was already committed. The manifest a
    /// live folder needs is already there: `create_with_meta` wrote it, status
    /// `recording`, before the sidecar spawned, which is precisely what the
    /// recovery pass keys off. The price is that a crash mid-session loses the
    /// host-clock PTS bounds of the segments it had closed (they exist only in
    /// the events, and the recovery reconcile rebuilds the ledger from the files
    /// on disk); the segments themselves are never at risk, and FR-146 makes
    /// that trade deliberately.
    ///
    /// Best-effort, like every write this sink makes: a failure is LOGGED ONLY.
    /// It must never change `machine` state or force the snapshot to `Failed`,
    /// because the single-child start-guard keys off that snapshot and a false
    /// `Failed` would let a second `keeper-rec` child spawn.
    fn finalize(&mut self) {
        self.manifest
            .set_status(ManifestStatus::from_state(self.machine.state()));
        // Story 21.5: the wall-clock end stamp rides the terminal manifest write
        // (ISO-8601 with offset, host-owned clock).
        self.manifest
            .set_ended_at(chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false));
        // EVERY terminal rebuilds the segment list from disk — disk is
        // authoritative (final segment, DW-992 backfill, real sizes) — before the
        // final write.
        if let Err(error) = self.manifest.reconcile_from_dir() {
            tracing::warn!(
                %error,
                "recording manifest: terminal disk reconcile failed; \
                 writing the event-fed view instead"
            );
        }
        if let Err(error) = self.manifest.write() {
            tracing::warn!(
                %error,
                "recording manifest: the terminal atomic write failed; the folder stays \
                 exactly as the recovery pass will find it (session unaffected)"
            );
        }
        // Story 42.1: the session's row is completed here, from the manifest that
        // was just reconciled and written — so the row and the folder say the
        // same thing about the same session, and the row's `ended_ts` is the
        // stamp the manifest carries rather than a second clock read.
        //
        // `INSERT OR REPLACE` on the session id, so this REPLACES the start row
        // rather than adding a second. That also makes a duplicate finalize
        // harmless, but it is not a licence to send two: the sink cannot produce
        // one (the machine rejects a second terminal event, so `handle` returns
        // before reaching here), and this is the only call site.
        if let Some(archive) = self.archive.as_ref() {
            archive.finalized(&self.manifest);
        }
        // The session's own push, whatever the policy says: `SessionEnd`
        // publishes here and only here, `Immediate` has published every segment
        // already and this publishes the manifest's commit, `Window` waits for
        // its hours. Which of those happens is the engine's decision — this only
        // says the session is over.
        if let Some(sync) = self.sync.as_ref() {
            sync.port
                .request_push(&sync.profile_id, RecordingPushTrigger::SessionEnd);
        }
        // Story 42.4 (FR-142): the minute the recording stops is the entire
        // window in which anything will ever be written about it, so the note
        // stub is composed and written HERE — after the manifest, because it is
        // composed from the manifest's own reconciled facts rather than from a
        // second reading of the session, and last, because it is the only thing
        // in this method the session does not depend on. Best-effort throughout:
        // see `write_recording_note_stub` for why a note that cannot be written
        // is never a recording failure.
        note_stub_at_finalize(
            &self.manifest,
            self.sync.as_ref().map(|sync| sync.profile_id.as_str()),
        );
    }
}

/// Story 41.1's `PushPolicy`, reduced to the one question a sink asks of it.
///
/// **Exhaustive by construction — no `_` arm**, for the reason
/// `sync_ipc`'s error mapping has none: which answer a new policy variant gets is
/// a decision about someone's uplink, and `_` would make it silently.
#[cfg(desktop)]
impl From<&keeper_sync::profile::PushPolicy> for SessionPushPolicy {
    fn from(policy: &keeper_sync::profile::PushPolicy) -> Self {
        match policy {
            keeper_sync::profile::PushPolicy::Immediate => Self::PerSegment,
            keeper_sync::profile::PushPolicy::SessionEnd => Self::AtSessionEnd,
            keeper_sync::profile::PushPolicy::Window { .. } => Self::InQuietHours,
        }
    }
}

/// The engine seam itself (Story 41.5): a [`RecordingSyncPort`] over this
/// process's one `Engine` plus the Story 41.4 assertion tap.
///
/// Deliberately thin. Every method is a translation — an `Engine` call, its error
/// into a sentence, its outcome into the one fact a sink can act on — because the
/// decisions belong on one side or the other: the policy in the engine, the
/// swallowing in the sink.
#[cfg(desktop)]
struct EngineRecordingSync {
    engine: Arc<keeper_sync::engine::Engine>,
    /// Minted once, at session start. Cheap to clone and it outlives nothing: a
    /// send after the engine is gone is a dropped message, never a dangling
    /// handle (Story 41.4).
    tap: keeper_sync::engine::FinishedTap,
}

#[cfg(desktop)]
impl RecordingSyncPort for EngineRecordingSync {
    fn push_policy(&self, profile_id: &str) -> SessionPushPolicy {
        let profiles = match self.engine.list_profiles() {
            Ok(profiles) => profiles,
            Err(error) => {
                tracing::warn!(
                    %error,
                    profile = %profile_id,
                    "recordings sync: the push policy could not be read, so nothing is published until this session ends"
                );
                return SessionPushPolicy::default();
            }
        };
        profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .and_then(|profile| profile.recordings.as_ref())
            .map(|recordings| SessionPushPolicy::from(&recordings.push))
            .unwrap_or_default()
    }

    fn ensure_lfs_rule(&self, profile_id: &str, extension: &str) -> Result<bool, String> {
        self.engine
            .ensure_lfs_rule(profile_id, extension)
            .map_err(|error| error.to_string())
    }

    fn note_finished(&self, profile_id: &str, path: &Path) -> bool {
        // The tap warns on its own dropped assertion (Story 41.4), and a dropped
        // assertion costs a settle window and nothing else — there is nothing to
        // add here and nothing to handle.
        self.tap.note_finished(profile_id, path)
    }

    fn request_push(&self, profile_id: &str, trigger: RecordingPushTrigger) {
        let engine = Arc::clone(&self.engine);
        let profile_id = profile_id.to_owned();
        let engine_trigger = match trigger {
            RecordingPushTrigger::SegmentCommitted => {
                keeper_sync::engine::PushTrigger::SegmentCommitted
            }
            RecordingPushTrigger::SessionEnd => keeper_sync::engine::PushTrigger::SessionEnd,
        };
        // SPAWNED, never awaited: the caller is a driver sink on a capture path
        // and a push is an upload — of an object that can be gigabytes. The
        // driver task also folds every sidecar event into the live snapshot, so
        // awaiting here would put the recording's own status behind the network.
        tauri::async_runtime::spawn(async move {
            match engine
                .push_recordings_if_due(&profile_id, engine_trigger)
                .await
            {
                Ok(true) => tracing::info!(
                    profile = %profile_id,
                    ?trigger,
                    "recordings sync: the profile pushed"
                ),
                Ok(false) => tracing::debug!(
                    profile = %profile_id,
                    ?trigger,
                    "recordings sync: the push policy says not now, so the commits stay local until it does"
                ),
                Err(error) => tracing::warn!(
                    %error,
                    profile = %profile_id,
                    ?trigger,
                    "recordings sync: the push was refused; the segments are committed locally and a later push publishes them"
                ),
            }
        });
    }

    fn path_durability(&self, profile_id: &str, path: &Path) -> Result<SegmentDurability, String> {
        self.engine
            .path_durability(profile_id, path)
            .map(SegmentDurability::from)
            .map_err(|error| error.to_string())
    }
}

/// The engine's own durability reading, mirrored into the platform-free shape
/// (Story 41.6) — a field-for-field translation and nothing more, for the same
/// reason [`SessionPushPolicy`]'s `From` is a translation: the decision (which
/// state these four facts add up to) belongs in exactly one place, and that
/// place is [`durability_state`], which compiles on a machine with no engine.
#[cfg(desktop)]
impl From<keeper_sync::engine::PathDurability> for SegmentDurability {
    fn from(facts: keeper_sync::engine::PathDurability) -> Self {
        Self {
            committed: facts.committed,
            pushed: facts.pushed,
            verified: facts.verified,
            problem: facts.problem,
        }
    }
}

/// This process's engine seam for a recording session, or `None` when there is no
/// engine to be had (Story 41.5).
///
/// `crate::sync::engine` caches, so this is a slot read on every start after the
/// first. A failure means no usable `git` (AD-41) — a degrade, not an error: the
/// session records to disk, the destination resolution has already said why
/// nothing is synced, and Record still works on a machine that never had git.
#[cfg(desktop)]
fn recording_sync_port(platform: &Arc<dyn Platform>) -> Option<Arc<dyn RecordingSyncPort>> {
    match crate::sync::engine(Arc::clone(platform)) {
        Ok(engine) => {
            let tap = engine.finished_tap();
            Some(Arc::new(EngineRecordingSync { engine, tap }))
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "recordings sync: there is no sync engine on this machine, so this session is recorded to disk only"
            );
            None
        }
    }
}

/// iOS links no `keeper-sync` at all, so there is no engine to seam onto — and no
/// synced destination either ([`destination_profile_table`] answers with an empty
/// table there), which is why this `None` is never even consulted.
#[cfg(not(desktop))]
fn recording_sync_port(_platform: &Arc<dyn Platform>) -> Option<Arc<dyn RecordingSyncPort>> {
    None
}

// --- Story 42.1: a session is a row --------------------------------------

/// The archive seam for one recording session (Story 42.1): the four writes the
/// recording path makes into `archive.db`, and nothing else.
///
/// A trait rather than a bare [`keeper_core::archive::ArchiveHandle`], for the
/// same reason [`RecordingSyncPort`] is one: this story's acceptance criteria are
/// COUNTS — one insert per start, one row per closed segment, one completion per
/// finalize, one update per durability MOVE — and a count is only assertable
/// against something a test can hold. The real implementation forwards to the
/// app's one serialized writer; a test's forwards to a `Vec`.
///
/// Every method returns `()`, and that is the contract rather than an omission:
/// the index is a cache of what the session folders already say, so nothing it
/// does may reach a recorder. A closed channel, a full queue and a failed write
/// are all logged inside the writer half and never travel back up here.
trait RecordingArchivePort: Send + Sync {
    /// The session's row at start: identity, place, `started_ts`.
    fn record_started(&self, row: RecordingRow);
    /// One closed segment's row.
    fn record_segment(&self, row: RecordingSegmentRow);
    /// The same session row, completed at finalize (`INSERT OR REPLACE` on the
    /// session id, so this replaces the start row rather than adding a second).
    fn record_finalized(&self, row: RecordingRow);
    /// The session's durability, sent when (and only when) 41.6's floor climbed.
    fn record_durability(&self, session_id: &str, state: RecordingDurabilityState);
    /// The session's new home after a Story 40.4 retitle moved the folder. Only
    /// the path travels: the row is keyed on identity, and the codec and frame
    /// rate it also holds live in no manifest, so a retitle has nothing else
    /// truthful to say.
    fn record_moved(&self, session_id: &str, relative_path: &str);
}

/// The real seam: the app's single serialized archive writer (Story 42.1).
///
/// Four forwards and nothing else. [`keeper_core::archive::ArchiveHandle`] is
/// already non-blocking (an unbounded channel) and already swallows a closed
/// channel with an ids-only log line, so there is no decision left for this
/// layer to make — which is exactly why the recording path may call it from a
/// driver task that is also folding sidecar events.
struct WriterChannelArchive(keeper_core::archive::ArchiveHandle);

impl RecordingArchivePort for WriterChannelArchive {
    fn record_started(&self, row: RecordingRow) {
        self.0.recording_started(row);
    }

    fn record_segment(&self, row: RecordingSegmentRow) {
        self.0.recording_segment(row);
    }

    fn record_finalized(&self, row: RecordingRow) {
        self.0.recording_finalized(row);
    }

    fn record_durability(&self, session_id: &str, state: RecordingDurabilityState) {
        self.0.recording_durability(session_id, state);
    }

    fn record_moved(&self, session_id: &str, relative_path: &str) {
        self.0
            .recording_moved(session_id.to_owned(), relative_path.to_owned());
    }
}

/// This process's archive seam, or `None` when there is no `archive.db` to write
/// into (Story 42.1).
///
/// The handle comes from [`AccountManager`], which opened the ONE writer for this
/// process at construction. The recording path deliberately has no handle of its
/// own: a second one would mean a second connection to `archive.db`, and one
/// writer is the whole premise of the archive's design.
fn recording_archive_port(state: &AppState) -> Option<Arc<dyn RecordingArchivePort>> {
    match state.accounts.archive() {
        Some(handle) => Some(Arc::new(WriterChannelArchive(handle))),
        None => {
            // Not an error and not a degrade worth a second line per session:
            // the database is derivable from the folders, so an unindexed
            // session is one `rebuild_from_disk` away from being indexed.
            tracing::warn!(
                "recordings archive: this app has no archive database open, so this session records without being indexed"
            );
            None
        }
    }
}

/// Repoint a retitled session's row at the folder it just moved to (Story 42.1,
/// matrix row 11).
///
/// A retitle is not a session, so it does not open a [`RecordingArchiveSession`]:
/// it has no codec, no frame rate and no live durability to speak for, and the
/// only thing that changed is where the folder is. Sending the path alone is
/// what keeps a retitle from writing nulls over the two columns that exist in no
/// manifest.
///
/// Silent about a session outside the destination root — the retitle command
/// already refused that case before the move, so reaching it here would mean the
/// root moved under us mid-call, and an index that declines to store an absolute
/// path has nothing to add to a log the rename already wrote.
#[cfg(desktop)]
fn index_retitled_session(
    state: &AppState,
    root: &Path,
    destination: &Path,
    manifest: &SessionManifest,
) {
    let Some(port) = recording_archive_port(state) else {
        return;
    };
    index_retitled_session_on(port.as_ref(), root, destination, manifest);
}

/// [`index_retitled_session`] with the seam passed in rather than resolved — the
/// whole decision, and the half worth testing.
///
/// Split out for the reason the rest of this story's seams are: the behaviour is
/// "which id, which path, and when nothing is sent at all", and none of that
/// needs an `AppState`, a registry or an `archive.db` to be true.
fn index_retitled_session_on(
    port: &dyn RecordingArchivePort,
    root: &Path,
    destination: &Path,
    manifest: &SessionManifest,
) {
    let Some(relative_path) = relative_session_path(root, destination) else {
        tracing::debug!(
            destination = %destination.display(),
            "recordings archive: a retitled session outside the destination root is not repointed (a row may hold no absolute path)"
        );
        return;
    };
    // The same identity rule the row was written under, so the update finds the
    // row the start created: `meta.session_id` when Story 40.3 minted one, and
    // the path-derived fallback otherwise. For a legacy session the fallback is
    // derived from the NEW path and therefore matches nothing — see the caller.
    let session_id = manifest
        .meta
        .as_ref()
        .and_then(|meta| meta.session_id.clone())
        .unwrap_or_else(|| fallback_session_id(&relative_path));
    port.record_moved(&session_id, &relative_path);
}

/// The archive half of one recording session, resolved ONCE at start and shared
/// by the driver task's sink and the status path's durability reader (Story
/// 42.1).
///
/// It carries what every row needs and the recording path would otherwise
/// re-derive per write: the session's identity, the destination root that every
/// stored path is relative to, and the two encode settings `manifest.json` does
/// not record. Session-captured for AD-25's reason — a mid-session settings edit
/// must not change what a running session's rows say about where it went or how
/// it was encoded.
struct RecordingArchiveSession {
    /// The writer seam itself.
    port: Arc<dyn RecordingArchivePort>,
    /// `<device ULID>-<session ULID>` (Story 40.3): the row's key, and the reason
    /// a retitle can move the folder without orphaning the row. Held because a
    /// durability update names the session and nothing else — the row writes take
    /// it from the manifest, which is where it also came from.
    session_id: String,
    /// The destination root every stored path is relative to. FR-145's rule
    /// extended to the index: a relative row survives the folder being retitled
    /// and the tree being cloned onto another machine.
    root: PathBuf,
    /// Which kind of destination that root is — `"folder"` or `"profile"`.
    root_kind: &'static str,
    /// The destination profile, when the root is one.
    profile_id: Option<String>,
    /// The video codec this session encodes with and the frame rate it captures
    /// at. Carried because `manifest.json` records NEITHER — they are settings
    /// this start read, and the row is the only place they are ever written
    /// down. (A rebuild can only reproduce what the folders say, so it sends
    /// them as `None` — and the write preserves whatever the live path already
    /// stored rather than nulling it, which is what keeps these two facts alive
    /// across a reindex.)
    codec: String,
    fps: u32,
}

impl RecordingArchiveSession {
    /// Open the archive half from the resolved destination and the identity the
    /// start just minted.
    fn open(
        port: Arc<dyn RecordingArchivePort>,
        session_id: String,
        destination: &RecordingDestination,
        codec: String,
        fps: u32,
    ) -> Self {
        Self {
            session_id,
            root: destination.root.clone(),
            // Exhaustive, no `_` arm: which word a new destination kind is filed
            // under is a decision about how a person will later find their
            // recordings, and `_` would make it silently.
            root_kind: match destination.kind {
                RecordingDestinationKind::Folder => "folder",
                RecordingDestinationKind::Profile => "profile",
            },
            profile_id: destination.profile_id.clone(),
            port,
            codec,
            fps,
        }
    }

    /// Build this session's row from the manifest as it currently stands.
    ///
    /// The manifest is the whole input on purpose: [`RecordingRow::from_manifest`]
    /// is the SAME constructor `rebuild_from_disk` calls after reading a
    /// `manifest.json` off disk, so a live row and a rebuilt row are identical
    /// rather than merely similar. Only the two things a manifest cannot carry are
    /// set afterwards.
    ///
    /// `None` when the folder is not under the destination root: no column may
    /// hold an absolute path, and a row that cannot say where the session is would
    /// be worse than no row.
    fn row(&self, manifest: &SessionManifest) -> Option<RecordingRow> {
        let relative_path = self.relative(manifest.folder())?;
        let mut row = RecordingRow::from_manifest(
            manifest,
            relative_path,
            self.root_kind,
            self.profile_id.as_deref(),
        );
        row.codec = Some(self.codec.clone());
        row.fps = Some(self.fps);
        // `durability` is left exactly as the constructor set it (`local`). The
        // floor lives on the status path (Story 41.6), so the sink cannot know how
        // far this session got — and does not have to: `upsert_recording` applies
        // the same never-regressing floor, so a session that reached `pushed` is
        // not walked back by its own finalize.
        //
        // `width`/`height` are left null for a plainer reason: nothing in this app
        // knows a session's encoded frame size. The sidecar never reports it and
        // the manifest has no video block, so the honest answer is "unknown" until
        // a later story probes the media itself.
        Some(row)
    }

    /// Send this session's row at start.
    fn started(&self, manifest: &SessionManifest) {
        if let Some(row) = self.row(manifest) {
            self.report_tags(&row);
            self.port.record_started(row);
        }
    }

    /// Send the same row completed, at finalize.
    fn finalized(&self, manifest: &SessionManifest) {
        if let Some(row) = self.row(manifest) {
            self.report_tags(&row);
            self.port.record_finalized(row);
        }
    }

    /// Hand this session's canonical tags to the tag tree's second producer
    /// (Story 42.5, FR-143).
    ///
    /// The row's own `tags_json`, decoded — never the manifest's text — so the
    /// tree counts exactly what the `recordings` row says and the two cannot
    /// disagree about a tag. Called on both write paths for the same reason
    /// `upsert_recording` is: a finalize replaces a start, and a session that
    /// gained or lost a tag between them must be re-credited, which
    /// [`keeper_core::notes::index::RecordingTagDelta::Upsert`] does by
    /// retracting the previous list first.
    ///
    /// Desktop-only because the tag tree is: `notes_vault` does not exist on
    /// iOS, where there is no notes surface and no sidebar to count into.
    fn report_tags(&self, row: &RecordingRow) {
        #[cfg(desktop)]
        crate::notes_vault::set_recording_tags(&row.session_id, &row.tags());
        #[cfg(not(desktop))]
        let _ = row;
    }

    /// Send one closed segment's row. `folder` is the session folder the ledger
    /// line's basename lives in; `closed_ts` is when this host saw it close.
    ///
    /// The row is built by [`RecordingSegmentRow::from_entry`] from the SAME
    /// [`SegmentEntry`] the ledger is about to record, so the row and the ledger
    /// line cannot disagree about a segment, and the relative path is joined the
    /// one way `rebuild_from_disk` joins it.
    fn segment(&self, folder: &Path, entry: &SegmentEntry, closed_ts: i64) {
        let Some(session_relative_path) = self.relative(folder) else {
            return;
        };
        let mut row =
            RecordingSegmentRow::from_entry(&self.session_id, &session_relative_path, entry);
        // The one thing a manifest cannot carry, so the one thing the constructor
        // leaves unset: a manifest records no per-segment close time, which is why
        // a rebuilt row has none and a live one does.
        row.closed_ts = Some(closed_ts);
        self.port.record_segment(row);
    }

    /// Send the session's new durability. Called only where the floor moved.
    fn durability(&self, state: RecordingDurabilityState) {
        self.port.record_durability(&self.session_id, state);
    }

    /// `path` relative to the destination root, or `None` with one debug line.
    ///
    /// Debug rather than warn, for [`RecordingSink::commit_finished_segment`]'s
    /// reason turned inside out: a session recording outside its own destination
    /// root is already visible in that path's logs, and the index has nothing to
    /// add beyond declining to store an absolute path.
    fn relative(&self, path: &Path) -> Option<String> {
        let relative = relative_session_path(&self.root, path);
        if relative.is_none() {
            tracing::debug!(
                session = %self.session_id,
                "recordings archive: this path is not under the destination root, so it is not indexed (a row may hold no absolute path)"
            );
        }
        relative
    }
}

/// Start the (at most one) recording session (Story 16.6 + 17.2 + 40.3,
/// FR-68/FR-69/FR-71/FR-127/FR-128/FR-145, AD-33/AD-37/AD-65/AD-73): render the
/// `recording.path_template` setting into a per-session folder under the
/// destination root, create it with its initial `recording` `manifest.json`,
/// spawn the driver task (fresh `keeper-rec` child; NDJSON events fold through
/// the platform-free session machine into the polled status snapshot AND the
/// segment ledger), and return the initial snapshot. The sidecar writes
/// `screen-0000.mov` (then `screen-0001.mov`, … on rotation) inside the folder.
///
/// The template is the namer, so the folder may NEST (`2026/2026-08-06 1536`),
/// the collision ordinal is the template's own `{seq}` wherever it put it, and
/// the path this creates is the path Settings → Recording previewed — same
/// renderer, same clock read, no second implementation (AD-65).
///
/// The session also gains its immutable identity here (`meta.sessionId`), because
/// the folder name is a label from story 40.4 onward and an archive row cannot be
/// keyed on a label.
///
/// A still-live prior session is an honest error — never two capture children.
/// Every fallible read happens BEFORE anything is created, and what an attempt
/// did create it removes on the way out ([`SessionScaffold`]), so a failed start
/// leaves no folder for the recovery pass to find. Pre-spawn failures funnel
/// through [`to_ipc_error`] (no session task exists yet); once the driver task
/// exists nothing it does can fail the session — the terminal manifest write, the
/// finished-path assertion and the push are all logged-only, and none of them
/// flips the live session to `failed` (the single-child start-guard keys off the
/// snapshot).
///
/// Story 41.5: when the destination resolves to a recordings-flagged profile, the
/// session also carries a sync half — the profile's LFS rule is written here,
/// before the sidecar spawns, and every closed segment is asserted to that
/// profile from the sink. A plain folder carries none of it.
///
/// Story 41.7: when that profile lives on removable media that is not attached,
/// the start is REFUSED by name ([`destination_volume_refusal`]) before the
/// recovery pass and before any folder is created. Not a degrade to the plain
/// folder — see the comment at the call site.
#[tauri::command]
// The command mirrors the wire: each optional capture selection + the three
// optional meta fields arrive as separate IPC args (Tauri flattens the invoke
// payload) — grouping them into a struct here would only add a second shape
// for the same data.
#[allow(clippy::too_many_arguments)]
pub async fn recording_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    target: Option<RecordingTargetVm>,
    system_audio: Option<bool>,
    microphone_enabled: Option<bool>,
    microphone_device_id: Option<String>,
    camera_enabled: Option<bool>,
    camera_device_id: Option<String>,
    meta_title: Option<String>,
    meta_participants: Option<String>,
    meta_note: Option<String>,
    // Story 42.5: the metadata card's tag field, exactly as typed — one
    // comma-separated line, not a list. The split used to live in TypeScript
    // and the trim again here, which made the frontend a second place that
    // decided what a tag is; both now happen once, in `tags::split_list`.
    meta_tags: Option<String>,
    meta_custom: Option<Vec<keeper_core::recording::SessionMetaField>>,
) -> Result<RecordingStatusVm, IpcError> {
    // Story 19.2: the Audio card's ephemeral per-session toggle. `None` (no
    // explicit choice reached the command) preserves the 16.6 default-on path.
    let system_audio = system_audio.unwrap_or(true);
    // Story 19.3: the Audio card's ephemeral mic selection — off unless
    // explicitly enabled (`None` → `false`, the lazy-permission default), the
    // device id `None` → the system default input. The one resolved flag feeds
    // BOTH the sidecar wire (`SessionParams.microphone`) and the manifest
    // (`SessionDevices.microphone`), so an off session honestly records
    // `devices.microphone = false` and no mic track.
    let mic_on = microphone_enabled.unwrap_or(false);
    let microphone = mic_on.then_some(MicSelection {
        device_id: microphone_device_id,
        // Story 22.7: filled from the registry below, once the data dir is
        // resolved — the ephemeral card carries the device pick, the persisted
        // setting carries the processing.
        echo_cancellation: keeper_core::registry::RECORDING_ECHO_CANCELLATION_DEFAULT,
    });
    // Story 20.1: the Webcam card's ephemeral camera selection, resolved by
    // the identical rule — off unless explicitly enabled, device id `None` →
    // the system default camera. The one resolved flag feeds BOTH the sidecar
    // wire (`SessionParams.camera`) and the manifest (`SessionDevices.camera`),
    // so an off session honestly records `devices.camera = false`, writes no
    // `camera-####` file, and touches no Camera-TCC.
    let camera_on = camera_enabled.unwrap_or(false);
    let camera = camera_on.then_some(CameraSelection {
        device_id: camera_device_id,
    });
    // Story 19.1: map the picker's selected target into the manifest capture
    // target + the sidecar's video-target params. `None` (no picker selection)
    // preserves the 16.6 main-display default. A vanished application fails
    // cleanly at the sidecar (an honest `error` → `Failed`), never here.
    let (capture_target, target_display_id, application, audio_only) =
        resolve_capture_target(target);

    // Settings/destination are resolved BEFORE the `recording_run` start-guard so
    // the pre-record recovery scan below runs OUTSIDE that slot lock. `data_dir`
    // and `directory` are pure reads (no session side effect); the destination
    // create/probe/free-space gate still runs under the guard further down. Read
    // once, at Start time — never re-read mid-session (AD-25): a mid-session edit
    // persists and mirrors both surfaces but only affects the next Recording
    // Session.
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    // Story 41.2 + 41.5: the destination is a resolved DECISION, and the whole
    // decision is read HERE, once. `directory` is its root — what the template
    // renders under, what the disk guard probes — and the profile half is what
    // the driver task commits into. Re-resolving either per segment would let a
    // settings edit repoint a running session (AD-25).
    let destination = effective_recording_destination(&data_dir, &|need| {
        destination_profile_table(&state.platform, need)
    });
    // Story 41.7, AD-48: the destination is a synced folder on a pendrive that is
    // not in the port. Refuse — do NOT redirect. Story 41.2's three degrades all
    // mean "this destination stopped being a destination"; this one means "your
    // destination is fine and is not here", and a recording that quietly landed
    // on the boot disk instead would be the exact outcome Epic 41 refuses. The
    // refusal happens HERE, before the recovery pass (which writes manifests) and
    // long before `create_dir_all`, so nothing at all is created — and it names
    // the drive, because an `EPERM` on `/Volumes/merope/tgdrive/recordings` is
    // not something anyone can act on.
    if let Some(refusal) = destination_volume_refusal(&destination) {
        return Err(refusal);
    }
    let directory = destination.root.clone();
    // Best-effort pre-record recovery of any session a prior crash left stale
    // (Story 17.3, FR-73, AD-37): reconcile every orphaned `recording` manifest
    // under the recovery-scan lock (serialized against the detached startup pass
    // — both `write` the same fixed `.manifest.json.tmp` per folder); the
    // `is_active` predicate skips any reserved live folder. Since Story 40.3 the
    // template nests, so this is no longer a scan of the root's children but a
    // descendant walk — bounded by `RECOVERY_MAX_DEPTH` levels and
    // `RECOVERY_MAX_VISITS` directories, because the root is a folder the USER
    // chose. It runs BEFORE the `recording_run` start-guard is acquired, so that
    // bounded-but-real blocking `read_dir`/`stat` work never stalls
    // stop/quit/tray, which contend on that slot (recording_snapshot's
    // invariant: the slot is never held across blocking `read_dir`/`stat`). The
    // new session's own folder does not exist yet, so the scan cannot see it; a
    // recovery failure is logged in the core pass and must NEVER fail the start.
    {
        let _scan = plain_lock(&state.recovery_scan);
        let is_active =
            |folder: &Path| plain_lock(&state.reserved_recording_folders).contains(folder);
        let recovered = recover_orphaned_sessions(&directory, &is_active);
        if !recovered.is_empty() {
            tracing::info!(
                count = recovered.len(),
                "pre-record recovery marked orphaned session(s) recovered"
            );
        }
    }

    let mut guard = slot_lock(&state.recording_run);
    if let Some(run) = guard.as_ref() {
        let live = !matches!(
            status_lock(&run.status).state,
            RecordingUiState::Finalized | RecordingUiState::Recovered | RecordingUiState::Failed
        );
        if live {
            return Err(IpcError {
                code: IpcErrorCode::Internal,
                message: "a recording is already running".to_owned(),
                account_id: None,
                retriable: false,
            });
        }
    }

    // The destination pre-flight gate (Story 19.5, AD-33): the user-chosen (or
    // default `~/Movies/keeper`) folder is probed shell-side — exists-or-
    // creatable via `create_dir_all`, writable via a real probe-file
    // write+remove (metadata perms are unreliable on macOS), free space via
    // `fs4::available_space` — and the pure core `evaluate_destination`
    // decides. A rejection blocks HERE, before the collision-suffix loop /
    // `SessionManifest::create` / any sidecar spawn: no session folder is
    // created and no capture begins.
    let creatable_or_exists = std::fs::create_dir_all(&directory).is_ok() && directory.is_dir();
    let writable = creatable_or_exists && destination_writable(&directory);
    // An exists-but-unprobeable volume must not block capture on a broken
    // probe: an errored probe reads as "plenty" (only a real low figure
    // rejects). The 0 on the non-directory branch never surfaces — the pure
    // decision checks NotADirectory first.
    let free_bytes = if creatable_or_exists {
        fs4::available_space(&directory).unwrap_or_else(|e| {
            tracing::warn!("recording destination free-space probe failed: {e}");
            u64::MAX
        })
    } else {
        0
    };
    evaluate_destination(
        creatable_or_exists,
        writable,
        free_bytes,
        RECORDING_MIN_FREE_BYTES,
    )
    .map_err(|reason| {
        to_ipc_error(CoreError::Recording(RecordingError::DestinationInvalid {
            reason,
        }))
    })?;
    // Every fallible READ happens here, above anything that touches the
    // filesystem. It used to sit below the folder creation, which meant a
    // registry hiccup returned an error while leaving a `recording` manifest on
    // disk for the recovery pass to surface as an interrupted session that never
    // started (Story 17.5 + 19.5, FR-72). The getters default + clamp/normalize,
    // so a fresh install starts with the authored figures. Read once — a later
    // edit never mutates a running session; it applies to the next one.
    let segment_mb =
        keeper_core::registry::get_recording_segment_mb(&data_dir).map_err(to_ipc_error)?;
    let duration_cap_minutes = keeper_core::registry::get_recording_duration_cap_minutes(&data_dir)
        .map_err(to_ipc_error)?;
    let fps = keeper_core::registry::get_recording_fps(&data_dir).map_err(to_ipc_error)?;
    // Story 21.1/21.2: codec + capture scale, normalized by the registry reads,
    // carried as additive wire params (absent ⇒ h264 / 100 on older sidecars).
    let codec = keeper_core::registry::get_recording_codec(&data_dir).map_err(to_ipc_error)?;
    let scale_percent =
        keeper_core::registry::get_recording_scale_percent(&data_dir).map_err(to_ipc_error)?;
    // Story 22.7: the persisted echo-cancellation switch, read HERE like every
    // other setting — the sidecar binds the voice-processing unit once at Start,
    // so a later edit applies to the next session only. Folded into the mic
    // selection so the key can only ever reach a mic-on wire, and only when the
    // mic block itself is emitted.
    let echo_cancellation =
        keeper_core::registry::get_recording_echo_cancellation(&data_dir).map_err(to_ipc_error)?;
    let microphone = microphone.map(|mic| MicSelection {
        echo_cancellation,
        ..mic
    });
    // Story 40.3: the template names the session, and the EFFECTIVE template is
    // always concrete — absent, blank and unparseable all degrade to
    // `DEFAULT_TEMPLATE` on read, so a start can never fail on what is stored.
    // The parse below therefore cannot fail in practice; it is mapped rather
    // than unwrapped because a start refusing with 40.1's own sentence is still
    // better than a panic if that invariant ever breaks.
    let template_source = effective_path_template(&data_dir)?;
    let template = PathTemplate::parse(&template_source).map_err(|reason| {
        to_ipc_error(CoreError::Recording(RecordingError::TemplateInvalid {
            reason,
        }))
    })?;
    // ONE clock read for the whole start, and it is the same context the preview
    // builds (AD-65): a second `Local::now()` below this line could name a folder
    // in the next minute and make the card's promise false by a hair.
    let now = Local::now();
    // The folder-name title is the SAME trimmed value the manifest records, read
    // back off the block below rather than re-derived: a folder named from one
    // trim rule and a manifest written from another is a session whose name and
    // whose title disagree, and nothing on screen would show it.
    // Story 21.5 + 22.3: the optional user metadata, trimmed, with blank entries
    // dropped (a custom row needs a NAME; a blank value is legal) — plus the
    // session's immutable identity, which is why the block is now ALWAYS written.
    // It used to be omitted when the user typed nothing, and an omitted block
    // would mean a session with no id at all.
    //
    // Story 45.19: those rules moved into `SessionMeta::from_input`, whole and
    // unchanged, because the editor on the FINISHED session applies the same
    // form and had nowhere to read them from. Two copies of "is this field
    // blank" and "where does one tag end" is how a field starts round-tripping
    // differently depending on which surface last saved it.
    let session_id = mint_session_id(&data_dir)?;
    let session_meta = keeper_core::recording::SessionMeta::from_input(
        Some(session_id.clone()),
        &keeper_core::recording::SessionMetaInput {
            title: meta_title.as_deref(),
            participants: meta_participants.as_deref(),
            note: meta_note.as_deref(),
            // Story 42.5: one tokenisation, in the tag module, for the one field
            // whose separator is a comma. What lands in `manifest.json` is still
            // the user's own text — the canonical form is applied later, by
            // `RecordingRow::from_manifest`, on the way into the index. The
            // manifest says what they typed; the row says what it means.
            tags: meta_tags.as_deref(),
            custom: meta_custom.as_deref().unwrap_or(&[]),
        },
    );
    let title = session_meta.title.clone();
    let devices = SessionDevices {
        system_audio,
        // Story 19.3: the mic leg is live — the manifest records whether this
        // session captures a microphone track.
        microphone: mic_on,
        // Story 20.1: the camera leg is live — whether this session writes the
        // separate `camera-####.mov` files.
        camera: camera_on,
    };
    let started_at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let (manifest, folder, reservation) = create_session_folder(
        &state.reserved_recording_folders,
        &directory,
        &template,
        &now,
        title.as_deref(),
        |folder| {
            SessionManifest::create_with_meta(
                folder,
                capture_target.clone(),
                devices,
                Some(session_meta.clone()),
                Some(started_at.clone()),
            )
        },
    )?;
    // Story 21.3: an audio-only session seeds `audio-0000.m4a`; the classic video
    // session keeps `screen-0000.mov` (17.1's `nextSegmentPath` then numbers
    // rotations either way). The extension beside it is this session's media
    // extension — the one the LFS rule has to cover.
    let (seed_name, media_extension) = session_media_seed(audio_only);
    // Story 41.5: open this session's sync half — destination profile, push
    // policy, LFS rule — BEFORE any sidecar exists, so the one working-tree
    // change it makes happens while nothing is recording (FR-137). `None` for a
    // plain folder and for a machine with no engine: both record to disk and
    // commit nothing, which is the degrade this epic is built around (NFR-34).
    let sync = begin_recording_sync(
        &destination,
        media_extension,
        recording_sync_port(&state.platform),
    );
    // Story 42.1: open this session's archive half and insert its row. HERE,
    // because a row needs both halves of what has just become true: the folder
    // exists (so there is a path to describe) and the destination is resolved (so
    // there is a root to describe it RELATIVE to, which is what lets the row
    // survive a retitle moving the folder or the tree being cloned onto another
    // machine). `None` when the app has no `archive.db` open — the index is a
    // cache of what the folders already say, so a session that cannot be indexed
    // still records, and `rebuild_from_disk` can add it later.
    //
    // `codec` is cloned rather than read back off `params` below because the row
    // must be sent before the driver task and the durability reader are built,
    // and both of them take the archive half.
    let archive = recording_archive_port(&state).map(|port| {
        let archive = Arc::new(RecordingArchiveSession::open(
            port,
            session_id.clone(),
            &destination,
            codec.clone(),
            fps,
        ));
        // The row is built from the manifest `create_with_meta` just wrote, not
        // from the pieces it was built out of, so the row and the folder say the
        // same thing about the same session — and so the row a rebuild would
        // derive from that file is the row written here.
        archive.started(&manifest);
        archive
    });
    // Story 41.6: the same seam, asked the other way round. The sink pushes
    // facts INTO the engine as it records; this reads one fact back out on the
    // status poll. Minted here, from the session's own resolved profile, so a
    // mid-session destination edit cannot repoint a running session's durability
    // at a profile it never wrote into (AD-25) — and `None` for a plain folder,
    // which is `local` by definition and asks nothing of anyone.
    let durability = sync.as_ref().map(|sync| {
        Arc::new(RecordingDurabilityReader::new(
            sync.profile_id.clone(),
            Arc::clone(&sync.port),
            archive.clone(),
        ))
    });
    let params = SessionParams {
        // Seeding `screen-0000.mov` lets 17.1's `nextSegmentPath` produce
        // `screen-0001.mov`, … inside the folder with no Swift change.
        output_path: folder.join(seed_name).to_string_lossy().into_owned(),
        // Story 19.1: an application target wins; otherwise the selected display
        // (or `None` = main display, the unchanged 16.6 path).
        display_id: target_display_id,
        application,
        system_audio,
        // Story 19.3: `Some` only when the mic source is enabled — the wire
        // then carries `micEnabled` (+ `micDeviceId` for a specific device).
        microphone,
        // Story 20.1: `Some` only when the webcam is enabled — the wire then
        // carries `cameraEnabled` (+ `cameraDeviceId` for a specific device).
        camera,
        segment_mb,
        // Minutes → seconds for the sidecar's `maxSegmentSeconds` (30 → 1800).
        max_segment_seconds: u32::from(duration_cap_minutes) * 60,
        // Story 19.5: the persisted frame rate (already normalized to
        // {10, 15, 30, 60} by the registry read) rides the wire as the
        // always-present `fps`.
        fps,
        codec,
        scale_percent,
        audio_only,
    };
    let status = Arc::new(Mutex::new(RecordingStatusVm {
        state: RecordingUiState::Preflight,
        segments_closed: 0,
        started_at_epoch_ms: None,
        // The session is a folder now (Story 17.2) — the VM points at it.
        output_path: Some(folder.to_string_lossy().into_owned()),
        error: None,
        // A new session starts clean (Story 19.4): the sticky warning slot is
        // reset here and ONLY here — the sink below never writes `None` back.
        warning: None,
        // Byte figures are read-time (filled by `recording_snapshot`) and the
        // cap is surfaced from the run (Story 18.3) — the stored snapshot the
        // driver keeps carries zeros, never inventing size or a cap here.
        on_disk_bytes: 0,
        current_segment_bytes: 0,
        segment_cap_mb: 0,
        // Derived at every read (Story 41.6), never folded from an event: the
        // stored snapshot carries the on-this-Mac reading and `with_disk_figures`
        // replaces it with the engine's answer on each poll.
        durability: RecordingDurabilityVm::local(),
    }));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let recorder = state.recorder.clone();
    let task_status = status.clone();
    // The platform port rides into the driver task for the Story 18.4 triad's
    // native-notification leg: the sink and the run-failure fallback both
    // dispatch through it on an `error`/`warning` onset.
    let task_platform = state.platform.clone();
    // The archive half rides in beside them (Story 42.1): the sink writes this
    // session's segment rows and its completion. The durability reader built
    // above holds the other `Arc` — one session, one row, two writers of it —
    // so this is the last use and moves rather than clones.
    let task_archive = archive;
    // The handle is stored into the run slot below (Story 18.2): aborting it is
    // the quit kill-timeout's force-kill lever (see `RecordingRun::driver`).
    let driver = tauri::async_runtime::spawn(async move {
        // Fold sidecar events through the platform-free machine into the shared
        // snapshot as they arrive (live — unlike `drive_session`'s buffered
        // replay), and hand every closed segment to the destination profile.
        let mut sink_state = RecordingSink {
            machine: RecordingSession::new(),
            manifest,
            status: task_status.clone(),
            platform: task_platform.clone(),
            sync,
            archive: task_archive,
        };
        let sink = Box::new(move |event: RecordingEvent| sink_state.handle(event))
            as Box<dyn FnMut(RecordingEvent) + Send>;

        let outcome = recorder
            .run_session(
                params,
                async move {
                    // A dropped sender (stop after terminal) must also resolve.
                    let _ = stop_rx.await;
                },
                sink,
            )
            .await;

        // A run failure (spawn fault, non-zero exit, unsupported) that did not
        // already surface as a terminal event becomes an honest failed snapshot —
        // with the Story 18.4 fault notification fired on onset (deduped against
        // the sink path inside `fail_recording_snapshot`).
        if let Err(error) = outcome {
            fail_recording_snapshot(&task_status, task_platform.as_ref(), error.to_string());
        }

        // The session can no longer write anything: release the live-folder
        // reservation (Story 17.3), so a later recovery pass may salvage this
        // folder if its manifest never reached a terminal status (a mid-
        // session write fault). Held to HERE — not the terminal event — so
        // the terminal reconcile+write in the sink above always completes
        // before the folder becomes recoverable. An aborted driver (quit
        // kill-timeout) drops the future, which drops this guard the same way.
        drop(reservation);
    });

    // Story 18.5: the live disk-space guard. A ~1 Hz task probes the
    // destination volume's free space (the same `fs4` probe as the pre-start
    // gate, fail-open on error) while THIS session is live, lets the pure core
    // policy plan at most one warn and one graceful stop per session, and
    // executes via `apply_disk_guard_action`. Measurement and execution live
    // here in the shell; thresholds, latching, and copy live in keeper-core
    // (AD-33/AD-39).
    let guard_status = status.clone();
    let guard_platform = state.platform.clone();
    let guard_app = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let mut latch = DiskGuardLatch::default();
        loop {
            tokio::time::sleep(DISK_GUARD_POLL).await;
            // The guard lives exactly as long as its session: exit once the
            // run slot was cleared (acknowledge) or replaced by a newer
            // session (Arc identity — a fresh session runs its OWN guard with
            // a fresh latch), re-reading the session-captured volume each tick.
            let directory = {
                let app_state = guard_app.state::<AppState>();
                let slot = slot_lock(&app_state.recording_run);
                match slot.as_ref() {
                    Some(run) if Arc::ptr_eq(&run.status, &guard_status) => {
                        run.destination_dir.clone()
                    }
                    _ => return,
                }
            };
            // ... or once the session reached a terminal state: a finalized/
            // failed session must never warn, and the post-stop latch already
            // guarantees the floor leg cannot re-fire while finalizing.
            if status_lock(&guard_status).state.is_terminal() {
                return;
            }
            // Fail-open probe (the pre-start idiom): an errored `statvfs`
            // reads as "plenty" — never a warn or stop on a broken probe.
            let free_bytes = fs4::available_space(&directory).unwrap_or_else(|e| {
                tracing::debug!("recording disk guard: free-space probe failed (fail-open): {e}");
                u64::MAX
            });
            let action = plan_disk_guard_action(
                free_bytes,
                RECORDING_WARN_FREE_BYTES,
                RECORDING_MIN_FREE_BYTES,
                &mut latch,
            );
            apply_disk_guard_action(&guard_status, guard_platform.as_ref(), action, || {
                // The identical idempotent trigger the tray Stop / ⌘Q use —
                // the session finalizes on the normal graceful path, and a
                // racing user stop simply finds the one-shot already taken.
                stop_active_recording(&guard_app.state::<AppState>());
            });
        }
    });

    let snapshot = status_lock(&status).clone();
    *guard = Some(RecordingRun {
        stop_tx: Some(stop_tx),
        status,
        driver: Some(driver),
        // The meter denominator, captured from this session's settings read
        // (Story 18.3) — never re-read from the mutable store while live.
        segment_cap_mb: segment_mb,
        // The volume the disk guard probes (Story 18.5) — the resolved
        // destination the pre-start gate just validated.
        destination_dir: directory,
        // The session's durability reading (Story 41.6) — `None` for a plain
        // folder, which never asks the engine anything.
        durability,
    });
    Ok(snapshot)
}

/// Fire the graceful stop trigger of the live recording session, if any (Story
/// 16.6 + 18.1): the one-shot the driver task's `stop` future awaits. Shared
/// verbatim by the [`recording_stop`] command and the tray's **Stop Recording**
/// item, so both fire the identical idempotent trigger — a second stop (or a
/// stop after the session ended) is a no-op, never an error, never a kill.
pub(crate) fn stop_active_recording(state: &AppState) {
    let mut guard = slot_lock(&state.recording_run);
    if let Some(run) = guard.as_mut() {
        if let Some(tx) = run.stop_tx.take() {
            // A send failure means the driver task already ended — nothing to stop.
            let _ = tx.send(());
        }
    }
}

/// Read (clone) the current recording-session status snapshot (Story 16.6 +
/// 18.1) — the single authoritative figure the tray's ~1 Hz tick, the quit gate
/// and the tray's Reveal item render from. No session yet this app lifetime ⇒
/// the honest idle snapshot.
///
/// Blocking: the enrichment below stats every segment file. The `recording_status`
/// / `recording_acknowledge` commands take [`recording_snapshot_off_runtime`]
/// instead, which produces the identical figures without occupying the calling
/// thread; this synchronous form exists for the callers that have no runtime to
/// await on (the tray menu handler and `ExitRequested`).
pub(crate) fn recording_snapshot(state: &AppState) -> RecordingStatusVm {
    let Some((snapshot, segment_cap_mb, durability)) = live_snapshot(&state.recording_run) else {
        return RecordingStatusVm::idle();
    };
    with_disk_figures(snapshot, segment_cap_mb, durability.as_deref())
}

/// [`recording_snapshot`] for the `async` command path (Story 34.3, AD-34-5):
/// byte-identical figures, with the `read_dir`/`stat` half on the blocking pool
/// so a slow volume neither stalls the main thread (where macOS resolves
/// `startDragging`) nor holds a runtime worker. Since Story 41.6 the durability
/// read rides the same pool hop, for the same reason: it is a local index read,
/// but it is still a read.
async fn recording_snapshot_off_runtime(state: &AppState) -> Result<RecordingStatusVm, IpcError> {
    let Some((snapshot, segment_cap_mb, durability)) = live_snapshot(&state.recording_run) else {
        return Ok(RecordingStatusVm::idle());
    };
    off_async_runtime(move || with_disk_figures(snapshot, segment_cap_mb, durability.as_deref()))
        .await
}

/// The lock-held half of [`recording_snapshot`]: clone the driver-kept snapshot,
/// capture the session cap and take a handle on the session's durability reader,
/// releasing the slot before returning. `None` ⇒ no session this app lifetime.
///
/// Split from the disk half so "never hold the `recording_run` slot across
/// blocking `read_dir`/`stat` syscalls" (Story 18.3 — a slow/unreadable volume
/// must not stall `stop`/`start`/quit, which contend on that slot) is enforced by
/// the signature rather than by comment, and so the async path can keep the
/// non-`Send` guard entirely on its own thread and hand owned values to the pool.
/// The durability reader is `Arc`'d out for exactly that rule: asking the engine
/// is a read too, and it must not happen under this lock either.
/// Takes the slot rather than the whole state ([`acknowledge_recording_slot`]'s
/// convention) so it is unit-testable without an `AppState`.
fn live_snapshot(
    slot: &Mutex<Option<RecordingRun>>,
) -> Option<(
    RecordingStatusVm,
    u32,
    Option<Arc<RecordingDurabilityReader>>,
)> {
    let guard = slot_lock(slot);
    let run = guard.as_ref()?;
    // Bind the clone first so the transient `status` MutexGuard drops before
    // `guard` (both released as this returns).
    let snapshot = status_lock(&run.status).clone();
    Some((snapshot, run.segment_cap_mb, run.durability.clone()))
}

/// The disk half of [`recording_snapshot`]: enrich the driver-kept snapshot with
/// the read-time byte figures and the session-captured cap (Story 18.3), so the
/// tray and the in-app banner render byte-identical size/segment/meter figures
/// from this one shared read. The driver never maintains these on the stored
/// snapshot; they are filled here best-effort — a missing/unreadable folder simply
/// yields 0 (never an error).
///
/// Story 41.6 adds the durability reading on the same terms: DERIVED here, on the
/// snapshot the surface already polls, so it cannot go stale or disagree with the
/// engine, and never stored anywhere. A `None` reader is a plain-folder
/// destination — `local`, and no engine is asked at all, because there is no
/// profile and therefore no further promise to make.
fn with_disk_figures(
    mut snapshot: RecordingStatusVm,
    segment_cap_mb: u32,
    durability: Option<&RecordingDurabilityReader>,
) -> RecordingStatusVm {
    if let Some(folder) = snapshot.output_path.as_deref() {
        let folder = Path::new(folder);
        snapshot.on_disk_bytes = session_bytes_on_disk(folder);
        snapshot.current_segment_bytes = current_segment_bytes_on_disk(folder);
    }
    snapshot.segment_cap_mb = segment_cap_mb;
    // The folder is read from the snapshot rather than captured at start, so a
    // retitle (Story 40.4, `repoint_recording_slot_output`) moves the question
    // with the session — the same reason the byte figures above take it from
    // there. No folder yet ⇒ nothing to ask about, and `local` is the honest
    // answer for a session that has written nothing.
    snapshot.durability = match (durability, snapshot.output_path.as_deref()) {
        (Some(reader), Some(folder)) => reader.read(Path::new(folder)),
        _ => RecordingDurabilityVm::local(),
    };
    snapshot
}

/// Collapse the engine's four local facts into the one state a person reads
/// (Story 41.6, FR-138). The single definition of the ranking — the banner, the
/// tray and the tests all come through here, so they cannot disagree.
///
/// Deliberately ordered strongest-first: `verified` implies `pushed` implies
/// `committed` in the engine's own bookkeeping, and reading the strongest true
/// fact means a partially-updated set can only ever be optimistic by one rung,
/// never nonsense.
fn durability_state(facts: &SegmentDurability) -> RecordingDurabilityState {
    if facts.verified {
        RecordingDurabilityState::Verified
    } else if facts.pushed {
        RecordingDurabilityState::Pushed
    } else if facts.committed {
        RecordingDurabilityState::Committed
    } else {
        RecordingDurabilityState::Local
    }
}

/// One session's durability reading, kept as a FLOOR (Story 41.6, FR-138,
/// NFR-34).
///
/// **Why a floor.** The line answers "would what I have recorded survive?", and
/// that is a question about the worst case of everything already captured, not
/// about the newest segment. A four-hour session pushes segment 3 and is still
/// settling segment 4; reporting segment 4's `local` would walk the banner
/// backwards and tell the person their recording became less safe, which is the
/// opposite of what happened. So the state only ever climbs: `floor.max(observed)`.
///
/// The `detail` does NOT floor. It is the current reason publication is stuck,
/// so it appears when a push is refused and disappears when a later one
/// succeeds — a reason that latched forever would outlive the problem it names.
///
/// **Why it degrades instead of failing.** A transient engine read failure on a
/// ~1 Hz poll must not blank the line, turn `pushed` back into `local`, or fail
/// the command: capture never degrades (NFR-34), and neither does reading about
/// it. The failure keeps the last known answer and spends exactly one `warn`
/// line per outage — [`Self::degraded`] latches so an hour of failures is one
/// line, not 3600, and clears on the next success so a NEW outage is heard.
struct RecordingDurabilityReader {
    /// The destination profile every question names.
    profile_id: String,
    /// The engine seam — the same one this session's sink commits through.
    port: Arc<dyn RecordingSyncPort>,
    /// The highest state this session has reached, plus the current reason
    /// publication is stuck. Interior-mutable because the status read takes
    /// `&self`: the reader is shared with the slot and read from the blocking
    /// pool.
    floor: Mutex<RecordingDurabilityVm>,
    /// Whether the current outage has already been logged.
    degraded: AtomicBool,
    /// The archive half of this session (Story 42.1), or `None` when the app has
    /// no `archive.db` open. The same [`Arc`] the sink holds: the row the sink
    /// created is the row this updates.
    archive: Option<Arc<RecordingArchiveSession>>,
}

impl RecordingDurabilityReader {
    /// Open a reader over one session's destination profile. The floor starts at
    /// `local`, which is exactly true: nothing has been committed yet.
    fn new(
        profile_id: String,
        port: Arc<dyn RecordingSyncPort>,
        archive: Option<Arc<RecordingArchiveSession>>,
    ) -> Self {
        Self {
            profile_id,
            port,
            floor: Mutex::new(RecordingDurabilityVm::local()),
            degraded: AtomicBool::new(false),
            archive,
        }
    }

    /// Ask the engine about `folder` and fold the answer into the floor,
    /// returning what the surface should show. Never fails.
    fn read(&self, folder: &Path) -> RecordingDurabilityVm {
        let answer = self.port.path_durability(&self.profile_id, folder);
        let mut floor = plain_lock(&self.floor);
        match answer {
            Ok(facts) => {
                if self.degraded.swap(false, Ordering::Relaxed) {
                    tracing::info!(
                        profile = %self.profile_id,
                        "recordings durability: the engine is answering again"
                    );
                }
                let observed = durability_state(&facts);
                if observed > floor.state {
                    floor.state = observed;
                    // Story 42.1: the index learns the advance HERE, and only
                    // here. This `>` IS the floor moving — it is the single
                    // assignment in the app that changes a live session's
                    // durability, so entering this branch is how the send site
                    // knows the state actually climbed rather than merely being
                    // observed again. A ~1 Hz poll of a settled session takes the
                    // other path and writes nothing; a session that walks
                    // local → committed → pushed sends exactly three updates.
                    if let Some(archive) = self.archive.as_ref() {
                        archive.durability(observed);
                    }
                }
                floor.detail = facts.problem;
            }
            // The last known state IS the answer here. Anything else would be a
            // worse lie than a slightly stale truth: `local` after `pushed`
            // claims the recording got less safe, and an error on the poll path
            // would blank a banner over a read that will very likely succeed a
            // second from now.
            Err(reason) => {
                if !self.degraded.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        %reason,
                        profile = %self.profile_id,
                        state = ?floor.state,
                        "recordings durability: the engine could not be read, so the last known state stands"
                    );
                }
            }
        }
        floor.clone()
    }
}

/// How long a confirmed quit waits for the live recording session to reach a
/// terminal state before force-killing the sidecar (Story 18.2). An authored
/// default — product sign-off at phase release, not an architecture constant.
const QUIT_FINALIZE_TIMEOUT: Duration = Duration::from_secs(10);

/// How often [`finalize_within`] re-reads the shared status snapshot while the
/// quit waits (Story 18.2): short enough that a normal finalize adds well under
/// a poll interval of latency to the quit.
const QUIT_FINALIZE_POLL: Duration = Duration::from_millis(100);

/// Bounded wait for the aborted driver task to actually unwind after a
/// kill-timeout (Story 18.2, review patch). `JoinHandle::abort` only *schedules*
/// cancellation; the sidecar's SIGKILL isn't delivered until a worker polls and
/// drops the `run_session` future (`kill_on_drop`). Blocking on the handle makes
/// that drop — and the kill — happen before quit proceeds, closing the race
/// where the process exits and orphans `keeper-rec`. Bounded so a wedged unwind
/// can't itself hang quit; in practice the handle resolves in milliseconds.
const QUIT_KILL_JOIN_TIMEOUT: Duration = Duration::from_secs(2);

/// The outcome of awaiting a recording finalize under a bound (Story 18.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinalizeOutcome {
    /// The session reached a terminal state (`finalized`/`recovered`/`failed`
    /// — or was already terminal) within the bound.
    Finalized,
    /// The session was still live when the bound elapsed (hung sidecar).
    TimedOut,
}

/// Poll the shared status snapshot until the session reaches a terminal state
/// or `timeout` elapses (Story 18.2). An already-terminal snapshot resolves
/// immediately without sleeping. The driver task keeps the snapshot current on
/// the runtime worker threads, so this can be `block_on`'d from the main
/// thread during `ExitRequested` (the established quit pattern).
async fn finalize_within(
    status: &Arc<Mutex<RecordingStatusVm>>,
    timeout: Duration,
    poll: Duration,
) -> FinalizeOutcome {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if status_lock(status).state.is_terminal() {
            return FinalizeOutcome::Finalized;
        }
        if tokio::time::Instant::now() >= deadline {
            return FinalizeOutcome::TimedOut;
        }
        tokio::time::sleep(poll).await;
    }
}

/// Stop and finalize the live recording session for a confirmed quit (Story
/// 18.2): fire the graceful `stop` trigger (the same idempotent one-shot as
/// Stop everywhere else), then await the session reaching a terminal state
/// under [`QUIT_FINALIZE_TIMEOUT`]. On timeout, abort the stored driver task —
/// dropping `run_session` drops the sidecar child, whose `kill_on_drop(true)`
/// force-terminates it — then block (bounded by [`QUIT_KILL_JOIN_TIMEOUT`]) on
/// the handle so that drop, and thus the kill, actually completes before quit
/// proceeds — so quit is never hung and `keeper-rec` is never orphaned. A quit
/// with no (or an already-terminal) session returns at once; force-kill fires
/// ONLY on the quit kill-timeout, never on a normal Stop.
pub(crate) fn finalize_recording_for_quit(state: &AppState) {
    stop_active_recording(state);
    let taken = {
        let mut guard = slot_lock(&state.recording_run);
        guard
            .as_mut()
            .map(|run| (run.status.clone(), run.driver.take()))
    };
    let Some((status, driver)) = taken else {
        return;
    };
    let outcome = tauri::async_runtime::block_on(finalize_within(
        &status,
        QUIT_FINALIZE_TIMEOUT,
        QUIT_FINALIZE_POLL,
    ));
    if outcome == FinalizeOutcome::TimedOut {
        tracing::warn!(
            timeout_secs = QUIT_FINALIZE_TIMEOUT.as_secs(),
            "quit: recording did not finalize within the kill-timeout; \
             aborting the driver task (sidecar force-terminated via kill_on_drop)"
        );
        if let Some(driver) = driver {
            driver.abort();
            // `abort()` only schedules cancellation — block (bounded) on the
            // handle so the `run_session` future is actually dropped (and its
            // `kill_on_drop` child SIGKILLed) before we let the process exit,
            // rather than racing process teardown and orphaning the sidecar.
            let _ = tauri::async_runtime::block_on(async {
                tokio::time::timeout(QUIT_KILL_JOIN_TIMEOUT, driver).await
            });
        }
    }
}

/// Request a graceful stop of the live recording session (Story 16.6): fires the
/// one-shot stop trigger; the sidecar finalizes the file (`stopping` →
/// `finalized` on the polled snapshot) and exits. Idempotent — a second stop (or
/// a stop after the session ended) is a no-op, never an error, never a kill.
///
/// `async` per AD-34-5: Stop is the click that most often precedes a window drag,
/// and a non-`async` command would fire the trigger on the main thread.
#[tauri::command]
pub async fn recording_stop(state: State<'_, AppState>) -> Result<(), IpcError> {
    stop_active_recording(&state);
    Ok(())
}

/// Read the current recording-session status snapshot (Story 16.6) — the poll
/// the Recording view's active-session UI renders from. Infallible in practice
/// (only a blocking-pool join failure can error).
///
/// `async` per AD-34-5: this is polled at ~1 Hz for the whole session and stats
/// every segment file on each tick, so on the main thread it would contend with
/// `startDragging` roughly once a second.
#[tauri::command]
pub async fn recording_status(state: State<'_, AppState>) -> Result<RecordingStatusVm, IpcError> {
    recording_snapshot_off_runtime(&state).await
}

/// Acknowledge (dismiss) a settled recording session's outcome (Story 18.4): a
/// terminal session (`finalized`/`recovered`/`failed`) is cleared back to idle —
/// dropping `error`/`warning` so the held tray error rendering restores/drops on
/// the next ~1 Hz tick and the banner error variant hides — while a **live**
/// session is a strict no-op (never a silent stop). Returns the fresh snapshot
/// either way (the idle default after a clear; the untouched live snapshot on
/// the no-op).
///
/// Shares [`acknowledge_recording_slot`] — the actual clear — with the tray's
/// **Dismiss Error** item, which reaches it through the synchronous
/// [`acknowledge_recording`]. `async` per AD-34-5: the snapshot it returns stats
/// the session folder.
#[tauri::command]
pub async fn recording_acknowledge(
    state: State<'_, AppState>,
) -> Result<RecordingStatusVm, IpcError> {
    acknowledge_recording_slot(&state.recording_run);
    recording_snapshot_off_runtime(&state).await
}

/// The tray's **Dismiss Error** path (Story 18.4): clear a terminal slot
/// ([`acknowledge_recording_slot`]) and return the resulting authoritative
/// snapshot. The [`recording_acknowledge`] command performs the same two steps
/// with the snapshot taken off the runtime (AD-34-5); the tray menu handler has no
/// runtime to await on, so it keeps the synchronous form.
pub(crate) fn acknowledge_recording(state: &AppState) -> RecordingStatusVm {
    acknowledge_recording_slot(&state.recording_run);
    recording_snapshot(state)
}

/// One sync profile, reduced to the facts a recordings destination turns on
/// (Story 41.2, FR-131; Story 41.7 added the volume, Story 46.10 the subfolder).
///
/// `keeper-sync`'s `SyncProfile` is a thirty-field type that only exists on
/// desktop; this is what the destination decision actually asks of it, mapped in
/// exactly one place ([`destination_profile_row`]). That is what keeps the
/// resolution and both of its refusals ordinary total functions: they compile on
/// iOS, where there is no engine to ask at all, and every degrade path in this
/// story is exercised in a test on a machine with no `git` — which is precisely
/// the machine those paths are about.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DestinationProfileRow {
    /// The profile's opaque id — what the destination setting persists.
    id: String,
    /// The profile's human name, for the refusal sentences and the VM.
    name: String,
    /// The synced folder itself. A plain destination anywhere inside it would be
    /// committed by this profile, which is the ambiguity the setter refuses.
    local_path: PathBuf,
    /// Where this profile's recordings live, or `None` when it does not say it
    /// holds them (Story 41.1's `recordings` block absent).
    ///
    /// **One field and not two** — see [`RecordingsPlace`]. Story 46.10 shipped
    /// the resolved root and the head it was joined from as two independent
    /// `Option`s that agreed only by construction; a row that named one without
    /// the other was representable and a reader consulting the head first would
    /// have read a head for a folder that holds no recordings (DW-196).
    recordings: Option<RecordingsPlace>,
    /// Whether watch mode is armed. A paused profile is neither a destination nor
    /// a collision — see [`enclosing_destination_profile`].
    enabled: bool,
    /// The removable media the folder lives on, and whether it is here right now
    /// (Story 41.7, AD-48) — `None` for a folder on a disk that is always there.
    ///
    /// Removability is the OPTION and the status is inside it, so "not removable,
    /// but its volume is absent" cannot be written down. Filled by
    /// [`scan_destination_volume`], which asks `keeper-sync`'s `volume::scan` —
    /// the one attachment test there is (Story 27.3): a marker at or above the
    /// folder, never an `exists()` on the mountpoint, which cannot tell an absent
    /// drive from a foreign one and cannot follow a stick re-mounted elsewhere.
    volume: Option<DestinationVolume>,
}

/// Where one profile's recordings live: the resolved root, and the
/// profile-relative head it was joined from (Story 46.10, collapsed in 47.5).
///
/// **The two travel together because they are one fact about one `recordings`
/// block.** [`destination_profile_row`] is the only place either is built and
/// it builds both from the same block; making that a single value is what stops
/// a later reader — or a later fixture — writing down a root with no head, or a
/// head for a profile that holds no recordings.
///
/// **The head is CARRIED, never recovered from the root.** A `strip_prefix` back
/// out of the resolved root would come back component-normalised, and
/// `20-media//sessions` and `20-media/sessions` are one root but two different
/// stored values. Only the stored one may be echoed back to `sync_profile_save`
/// by the Destination card's edit box.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordingsPlace {
    /// The RESOLVED recordings root, exactly as
    /// [`keeper_sync::SyncProfile::recordings_root`] composes it — asked of
    /// `keeper-sync` once and never reimplemented here.
    root: PathBuf,
    /// The profile-relative subfolder [`Self::root`] was joined FROM, trimmed
    /// exactly as the join trims, so what the card shows is what the join used.
    subfolder: String,
}

impl DestinationProfileRow {
    /// The resolved recordings root, for the readers that only ask *where*.
    ///
    /// Borrowing, so asking the question costs nothing and cannot hand a caller
    /// a root separated from its head.
    fn recordings_root(&self) -> Option<&Path> {
        self.recordings.as_ref().map(|place| place.root.as_path())
    }
}

/// A destination profile's removable volume: what it is called, and whether it
/// is attached (Story 41.7, AD-48).
///
/// The shell-side, `keeper-sync`-free shape of `volume::VolumeStatus`, for the
/// same reason [`DestinationProfileRow`] is the shell-side shape of
/// `SyncProfile`: the resolution and its refusal stay ordinary total functions
/// that compile on iOS and are asserted in tests with no engine, no `git` and no
/// pendrive.
///
/// **The STATUS is never cached.** It is re-scanned every time the profile table
/// is built — every settings read, every destination picker load, every
/// `recording_start`. A cached `Absent` is the one value that would outlive the
/// thing it describes: the drive goes back in the port and the app would keep
/// refusing until something invalidated an entry nobody would think to
/// invalidate. The scan itself is one `stat` of `.keeper-sync/volume.json` per
/// ancestor of the folder, which is cheaper than being wrong about a plugged-in
/// drive. The NAME is remembered, and only the name — see [`VOLUME_NAMES`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct DestinationVolume {
    /// What the volume calls itself: its marker's `label`, which
    /// `Engine::adopt_volume` set to the mount point's own name ("merope") the
    /// first time the volume was seen. Never sliced out of `local_path` — the
    /// mountpoint is the one part of a volume's identity that moves (Story 27.3),
    /// and a stick re-mounted elsewhere is the same stick with the same name.
    ///
    /// `None` when this build has never had the volume's marker in front of it:
    /// a detached drive carries its own name away with it, and inventing one from
    /// the path is exactly the guess this field refuses to make. The refusal and
    /// the card both have an unnamed phrasing for that case.
    name: Option<String>,
    /// Whether the volume is here right now.
    status: DestinationVolumeStatus,
}

/// Whether a destination's removable volume is attached (Story 41.7).
#[derive(Debug, Clone, PartialEq, Eq)]
enum DestinationVolumeStatus {
    /// The volume the profile is bound to is attached. Recording proceeds.
    Attached,
    /// No marker at or above the folder: the media is not here
    /// (`VolumeStatus::Absent`, which maps to `ProfileState::MediaAbsent`).
    Absent,
    /// Something is mounted where the profile's volume lives but is not provably
    /// that volume: a foreign marker (`VolumeStatus::Foreign` — a second stick at
    /// the first one's mountpoint), or a marker that would not read.
    ///
    /// Folded into one state because the two take the SAME action from the person
    /// holding the drive — look at what is actually plugged in — while `Absent`
    /// takes a different one. `detail` carries the specific reason into the
    /// refusal so the sentence is still precise.
    Unexpected { detail: String },
}

/// This machine's sync profiles, or the reason there are none to be had.
///
/// `Err` carries a sentence for a `warn` line rather than an [`IpcError`],
/// because "there is no usable `git` on this machine" is not a failure of
/// whatever asked: the destination degrades to the plain-folder answer and the
/// recorder keeps working (NFR-34). Only the settings WRITE turns it into a
/// refusal, and only for a submitted profile id it cannot verify.
type DestinationProfileTable = Result<Vec<DestinationProfileRow>, String>;

/// Why the destination resolution is asking for the profile table (Story 41.2),
/// which decides what the answer is allowed to COST.
///
/// The two are not interchangeable. An explicit choice is a promise the user
/// made, so resolving it may build the engine — a `keeper.db` read, a `git`
/// probe and a `sync.db` open — because recording somewhere other than the folder
/// they picked is not an option. The single-flagged-profile default is a
/// convenience, so it uses an engine only if one is already open: on a machine
/// with no usable `git` the engine can never be built, and `git_resolution`
/// deliberately does not cache a refusal (so a `brew install git` takes effect
/// without a relaunch), which would make every settings read — and every
/// keystroke of the template preview, which resolves the same root — pay a full
/// `PATH` search with a process spawn per candidate to be told the same thing
/// again. The cost of that asymmetry is one narrow window: between boot and
/// `start_supervisor` building the engine, the implicit default reads as the
/// folder answer. Nothing is persisted from a read, so the window costs a fresh
/// install's first pane paint and never a written destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileTableNeed {
    /// Resolving an explicitly stored profile id, or validating a submission.
    Chosen,
    /// Looking for the implicit default with neither key set.
    Default,
}

/// The recordings destination as a resolved DECISION rather than a path (Story
/// 41.2, FR-131, UX-DR47): the absolute root, which of the two settings keys
/// produced it, and the profile's id and name when a profile did.
///
/// Composed once, here, so no surface joins a `local_path` and a subfolder
/// itself: `recording_start`, the recovery scans, the palette's reveal and the
/// settings card all read the same answer, and none of them can disagree with
/// where the recorder actually writes.
struct RecordingDestination {
    /// The absolute root recordings land under, whichever choice is in force.
    root: PathBuf,
    /// Which choice produced [`Self::root`].
    kind: RecordingDestinationKind,
    /// The chosen profile's id, under [`RecordingDestinationKind::Profile`] only.
    profile_id: Option<String>,
    /// The chosen profile's name, under [`RecordingDestinationKind::Profile`]
    /// only. Resolved from the id on every read, never cached beside it, which is
    /// what makes a rename show up with the same root.
    profile_name: Option<String>,
    /// The chosen profile's removable media, under
    /// [`RecordingDestinationKind::Profile`] only and only when the folder is on
    /// any (Story 41.7).
    ///
    /// Carried on the DECISION rather than looked up again by whoever needs it,
    /// so the sentence the card prints and the refusal `recording_start` raises
    /// come from the same scan of the same volume. Read by
    /// [`destination_volume_refusal`]; every other caller only wants
    /// [`Self::root`] and is unaffected.
    volume: Option<DestinationVolume>,
}

/// The EFFECTIVE recordings destination (Story 41.2): the chosen sync profile's
/// recordings root when that choice still resolves, otherwise Story 19.5's plain
/// folder answer.
///
/// **Total on purpose, in both directions.** Every way a profile answer can fail
/// — the id names no profile, names a paused one, names one that no longer says
/// it holds recordings, or there is no engine to ask because this machine has no
/// usable `git` — degrades to the plain-folder answer and says why at `warn`. A
/// machine with no `git` still records, and a settings surface that refused to
/// load because of a stale id would leave the user with no way to fix the very
/// field that broke it. A `keeper.db` read failure degrades the same way for the
/// same reason; the sibling getters in [`read_recording_settings`] still surface
/// that fault, but the DESTINATION always has a concrete root.
///
/// Takes the profile table as a lazy closure rather than a platform, so it is
/// called at most once and only when a profile id is actually stored: building
/// the engine on a machine with no `git` re-searches `PATH` with a process spawn
/// per candidate (a refusal is deliberately never cached), and the settings read
/// runs on every visit to the Recording pane.
fn effective_recording_destination(
    data_dir: &Path,
    profiles: &dyn Fn(ProfileTableNeed) -> DestinationProfileTable,
) -> RecordingDestination {
    let stored_dir = match keeper_core::registry::get_recording_destination_dir(data_dir) {
        Ok(stored) => stored,
        Err(error) => {
            tracing::warn!(%error, "the recording destination folder could not be read; using the default folder");
            None
        }
    };
    let stored_profile = match keeper_core::registry::get_recording_destination_profile(data_dir) {
        Ok(stored) => stored,
        Err(error) => {
            tracing::warn!(%error, "the recording destination profile could not be read; using the folder answer");
            None
        }
    };
    resolve_recording_destination(stored_dir, stored_profile, data_dir, profiles)
}

/// Resolve the destination from the (already-fetched) persisted values.
///
/// The precedence, and it is deterministic rather than a guess:
///
/// 1. A stored profile id that still resolves WINS. When the folder key is also
///    set — a state the setter refuses to create, so a hand-edited `config.json`
///    is the only way to reach it — the win is announced at `warn` instead of
///    happening silently.
/// 2. Neither key set, and EXACTLY ONE enabled, recordings-flagged profile
///    exists ⇒ that profile, resolved, with nothing written. Flagging a folder as
///    holding recordings is not something this app does to itself: it is done
///    deliberately through `keeper-syncd`, and it is already the statement "the
///    recordings live here", so honouring it is reading the user's answer rather
///    than inventing one. Two or more is NOT a default — ambiguity resolved by
///    coin toss would put someone's recordings on a remote they did not pick.
/// 3. Everything else is the folder answer, and every degrade rule above wins
///    over the default: a single flagged profile that is paused, gone, or
///    unreadable is the folder answer with its `warn`.
///
/// Pure, so every row of the matrix that ends in a root is asserted without a
/// registry, an engine, or a `git`.
fn resolve_recording_destination(
    stored_dir: Option<String>,
    stored_profile_id: Option<String>,
    data_dir: &Path,
    profiles: &dyn Fn(ProfileTableNeed) -> DestinationProfileTable,
) -> RecordingDestination {
    let folder_key_set = stored_dir.is_some();
    let folder = RecordingDestination {
        root: resolve_destination_dir(stored_dir, data_dir),
        kind: RecordingDestinationKind::Folder,
        profile_id: None,
        profile_name: None,
        // The plain folder is wherever the owner pointed it; keeper knows of no
        // volume behind it, and claiming one would be inventing a fact.
        volume: None,
    };
    let Some(id) = stored_profile_id else {
        // An explicitly chosen folder is an answer; only the absence of BOTH keys
        // is the absence of one.
        if folder_key_set {
            return folder;
        }
        return default_recording_destination(folder, profiles);
    };
    if folder_key_set {
        tracing::warn!(
            profile = %id,
            "both recording destination keys are set; the synced folder wins and the next write clears the other"
        );
    }
    let rows = match profiles(ProfileTableNeed::Chosen) {
        Ok(rows) => rows,
        Err(reason) => {
            tracing::warn!(
                %reason,
                profile = %id,
                "the synced folders cannot be read, so the chosen one cannot be resolved; recording into the plain folder instead"
            );
            return folder;
        }
    };
    let Some(row) = rows.into_iter().find(|row| row.id == id) else {
        tracing::warn!(
            profile = %id,
            "the chosen synced folder is no longer set up on this machine; recording into the plain folder instead"
        );
        return folder;
    };
    if !row.enabled {
        tracing::warn!(
            profile = %id,
            "the chosen synced folder is paused, so nothing recorded there would be committed; recording into the plain folder instead"
        );
        return folder;
    }
    let Some(place) = row.recordings else {
        tracing::warn!(
            profile = %id,
            "the chosen synced folder no longer says it holds recordings; recording into the plain folder instead"
        );
        return folder;
    };
    // Story 41.7: an absent volume is NOT a fourth degrade. The three above mean
    // "this destination is not a destination any more"; a pendrive in a drawer
    // means "this destination is fine and is not here right now", and quietly
    // landing a recording somewhere other than where the card said is the one
    // outcome this epic must not add. So the volume rides along and
    // `recording_start` refuses on it — the resolution stays total, and the card
    // keeps naming the folder the owner actually chose.
    RecordingDestination {
        root: place.root,
        kind: RecordingDestinationKind::Profile,
        profile_id: Some(row.id),
        profile_name: Some(row.name),
        volume: row.volume,
    }
}

/// The destination when the owner has chosen nothing at all (Story 41.2, the
/// `tgdrive` case): the one folder that says it holds recordings, if there is
/// exactly one.
///
/// **Nothing is written.** This is a RESOLUTION, so the settings keys stay empty
/// and the first explicit choice writes as usual. That is what makes it honest:
/// the card renders the profile answer with its consequence because that IS where
/// a recording started now would land, and no render has silently redirected
/// anyone's recordings to a remote.
///
/// **Why exactly one.** With two flagged folders there is no default to be had —
/// choosing between them here would be a coin toss with a push at the end of it,
/// and the picker is right there. With none, the surface is exactly today's.
fn default_recording_destination(
    folder: RecordingDestination,
    profiles: &dyn Fn(ProfileTableNeed) -> DestinationProfileTable,
) -> RecordingDestination {
    let rows = match profiles(ProfileTableNeed::Default) {
        Ok(rows) => rows,
        Err(reason) => {
            tracing::debug!(
                %reason,
                "no synced folders to default to; recording into the plain folder"
            );
            return folder;
        }
    };
    let mut flagged = rows.into_iter().filter_map(|row| {
        let root = row.recordings.filter(|_| row.enabled)?.root;
        Some((row.id, row.name, root, row.volume))
    });
    let Some((id, name, root, volume)) = flagged.next() else {
        return folder;
    };
    if flagged.next().is_some() {
        tracing::debug!(
            "more than one synced folder holds recordings, so there is no default; recording into the plain folder until one is chosen"
        );
        return folder;
    }
    tracing::info!(
        profile = %id,
        "no destination has been chosen and exactly one synced folder holds recordings, so that is where recordings land"
    );
    // The implicit default carries its volume for the same reason the explicit
    // choice does: the card states this root, so a recording must land in it or
    // not happen. An unplugged default is refused, never redirected.
    RecordingDestination {
        root,
        kind: RecordingDestinationKind::Profile,
        profile_id: Some(id),
        profile_name: Some(name),
        volume,
    }
}

/// The EFFECTIVE destination ROOT for the callers that only need the folder —
/// `recording_start`, the recovery scans, the retitle root, the palette's reveal
/// and the template preview (Story 19.5, extended by Story 41.2).
///
/// Every one of them now sees the profile answer too, which is the whole point of
/// the story: a destination the settings card resolves to a synced folder and the
/// recorder resolves to something else would be two destinations. The platform is
/// threaded in for the engine the resolution may need, exactly as Story 40.4's
/// `sync_retitled_session` takes one.
pub(crate) fn effective_destination_dir(data_dir: &Path, platform: &Arc<dyn Platform>) -> PathBuf {
    effective_recording_destination(data_dir, &|need| destination_profile_table(platform, need))
        .root
}

/// This machine's sync profiles, reduced to [`DestinationProfileRow`]s.
///
/// Routed through `crate::sync` exactly as Story 40.4's `sync_retitled_session`
/// is: the engine cannot exist without a usable `git` (AD-41), which is a runtime
/// fact rather than a build flag, and its absence is an answer here rather than a
/// failure anyone must handle.
///
/// [`ProfileTableNeed`] decides whether the engine may be BUILT for this answer.
/// A chosen destination is worth the probe; the implicit default is worth only an
/// engine that is already open (`start_supervisor` opens one at boot on every
/// machine where sync works at all).
#[cfg(desktop)]
fn destination_profile_table(
    platform: &Arc<dyn Platform>,
    need: ProfileTableNeed,
) -> DestinationProfileTable {
    let engine = match need {
        ProfileTableNeed::Chosen => {
            crate::sync::engine(Arc::clone(platform)).map_err(|error| error.to_string())?
        }
        ProfileTableNeed::Default => crate::sync::engine_if_open()
            .ok_or_else(|| "no sync engine is open on this machine".to_owned())?,
    };
    let profiles = engine.list_profiles().map_err(|error| error.to_string())?;
    Ok(profiles.iter().map(destination_profile_row).collect())
}

/// iOS has no folder sync at all (`keeper-sync` is not even linked there), so
/// there is no synced folder to choose, none to default to, and none to collide
/// with. An empty table is the truth on that platform, not a degrade — which is
/// why this is `Ok`.
#[cfg(not(desktop))]
fn destination_profile_table(
    platform: &Arc<dyn Platform>,
    need: ProfileTableNeed,
) -> DestinationProfileTable {
    let _ = (platform, need);
    Ok(Vec::new())
}

/// The one place a `SyncProfile` becomes a [`DestinationProfileRow`], so "where
/// do this profile's recordings live" is asked of `keeper-sync` once
/// ([`keeper_sync::SyncProfile::recordings_root`]) and never reimplemented — and,
/// since Story 41.7, so is "and is that place plugged in".
#[cfg(desktop)]
fn destination_profile_row(profile: &keeper_sync::SyncProfile) -> DestinationProfileRow {
    DestinationProfileRow {
        id: profile.id.clone(),
        name: profile.name.clone(),
        local_path: profile.local_path.clone(),
        // The root and the head it was composed from, taken from the SAME block
        // in one expression, so the pair cannot describe two different profiles
        // and cannot be half-written (Story 46.10, DW-196). The join itself stays
        // `keeper-sync`'s: `recordings_root()` is the one definition of it.
        recordings: profile
            .recordings_root()
            .zip(profile.recordings.as_ref())
            .map(|(root, recordings)| RecordingsPlace {
                root,
                subfolder: recordings.subfolder.trim().to_owned(),
            }),
        enabled: profile.enabled,
        // Only a profile that says it is on removable media is scanned. For every
        // ordinary folder this is the whole cost of the feature: one boolean.
        volume: profile.removable.then(|| scan_destination_volume(profile)),
    }
}

/// Ask `keeper-sync` whether this removable profile's volume is attached (Story
/// 41.7, AD-48).
///
/// Delegates to `volume::scan`, which is the ONLY attachment test in the
/// codebase and must stay so. The tempting alternative — `local_path.exists()`,
/// or a probe of the mountpoint — is wrong in both directions Story 27.3 was
/// written about: it reads a second stick mounted at the first one's mountpoint
/// as "attached" (and would record a session into a stranger's disk), and it
/// reads the same stick re-mounted at a different mountpoint as "gone". The
/// marker travels with the filesystem; the path does not.
///
/// Every outcome is a [`DestinationVolume`] rather than an error, because the
/// caller is building a table that must not fail: an unreadable marker is a
/// reason to refuse a START, never a reason for the settings pane to fail to
/// load — which would leave the owner with no way to change the very destination
/// that broke it.
#[cfg(desktop)]
fn scan_destination_volume(profile: &keeper_sync::SyncProfile) -> DestinationVolume {
    use keeper_sync::volume::{self, VolumeStatus};

    match volume::scan(&profile.local_path, profile.volume_id.as_deref()) {
        Ok(VolumeStatus::Present { marker }) => DestinationVolume {
            // The marker is in front of us, so this is the moment — the only
            // moment — the volume's own name can be learned. Remember it, so the
            // refusal after it is unplugged can still say "merope".
            name: remember_volume_name(&marker.volume_id, &marker.label),
            status: DestinationVolumeStatus::Attached,
        },
        Ok(VolumeStatus::Absent) => DestinationVolume {
            name: recalled_volume_name(profile.volume_id.as_deref()),
            status: DestinationVolumeStatus::Absent,
        },
        Ok(VolumeStatus::Foreign { found_id }) => DestinationVolume {
            name: recalled_volume_name(profile.volume_id.as_deref()),
            status: DestinationVolumeStatus::Unexpected {
                detail: format!("a different volume ({found_id}) is mounted there"),
            },
        },
        Err(error) => DestinationVolume {
            name: recalled_volume_name(profile.volume_id.as_deref()),
            status: DestinationVolumeStatus::Unexpected {
                detail: error.to_string(),
            },
        },
    }
}

/// Volume ids to the labels their markers carried, learned as they are scanned
/// (Story 41.7).
///
/// **Why this exists.** A volume's name lives in its marker, at its mount root —
/// so the moment a drive is unplugged, the name goes with it, and the refusal
/// that most needs to say "merope is not attached" is exactly the one that can no
/// longer read it. Nothing else persists a label: `SyncProfile` binds a volume by
/// ULID (deliberately — a label is cosmetic and never matched on), and slicing a
/// name out of `local_path` is the guess Story 27.3 exists to forbid.
///
/// **What it may and may not hold.** Names only. Never a
/// [`DestinationVolumeStatus`] — memoizing "absent" is the one thing that could
/// make a replugged drive keep failing, and this map is deliberately incapable of
/// it. A label is also stable in a way a status is not: `VolumeMarker::ensure`
/// fills an empty label once and never renames a volume another profile named.
///
/// **Lifetime: this process.** Not persisted, not invalidated, not bounded by
/// time — it is bounded by the number of removable volumes one machine has ever
/// had attached during one run of the app, which is a handful of short strings.
/// A restart with the drive already out simply falls back to the unnamed
/// phrasing, which is honest; the alternative — persisting labels into the
/// registry — would be a second, staler place a volume is named.
#[cfg(desktop)]
static VOLUME_NAMES: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Learn `label` for `volume_id` and hand it back, or hand back what was already
/// learned when this marker carries no label of its own.
#[cfg(desktop)]
fn remember_volume_name(volume_id: &str, label: &str) -> Option<String> {
    let label = label.trim();
    let mut names = plain_lock(&VOLUME_NAMES);
    if label.is_empty() {
        // A marker minted somewhere nameless. Not a reason to forget a name an
        // earlier scan of the same volume did learn.
        return names.get(volume_id).cloned();
    }
    // A volume that has been relabelled keeps the newest name its marker gave;
    // the id, never the label, is what identity is bound to.
    let known = names.entry(volume_id.to_owned()).or_default();
    if known.as_str() != label {
        known.clear();
        known.push_str(label);
    }
    Some(known.clone())
}

/// What this process last saw the volume `volume_id` call itself — `None` for a
/// profile bound to no volume yet, or a volume this run has never had attached.
#[cfg(desktop)]
fn recalled_volume_name(volume_id: Option<&str>) -> Option<String> {
    let volume_id = volume_id?;
    plain_lock(&VOLUME_NAMES).get(volume_id).cloned()
}

/// The enabled profile whose folder CONTAINS `path`, deepest match first (Story
/// 41.2) — the collision the plain-folder refusal is about.
///
/// The shape of `crate::sync::profile_for_path` over the reduced rows, and it
/// skips disabled profiles for that function's reason and one more: the
/// resolution above degrades for a paused profile and the picker does not offer
/// one, so `enabled` means exactly one thing everywhere in this story — a paused
/// folder is neither a destination nor a collision. The trade-off is real and
/// accepted: a folder chosen inside a paused profile's tree becomes ambiguous if
/// that profile is ever resumed. The alternative refuses the folder while also
/// refusing the profile it names, which is a dead end from this surface.
fn enclosing_destination_profile<'a>(
    rows: &'a [DestinationProfileRow],
    path: &Path,
) -> Option<&'a DestinationProfileRow> {
    let mut deepest: Option<&'a DestinationProfileRow> = None;
    for row in rows {
        if !row.enabled || !path.starts_with(&row.local_path) {
            continue;
        }
        let deeper = deepest.is_none_or(|held| {
            row.local_path.components().count() > held.local_path.components().count()
        });
        if deeper {
            deepest = Some(row);
        }
    }
    deepest
}

/// Resolve the effective destination folder from the (already-fetched) persisted
/// value. Only an ABSOLUTE persisted path is honored; a relative value (e.g. a
/// hand-edited or corrupted `recording.destination_dir` row) is rejected in
/// favor of the default, so the VM's "always a concrete absolute folder"
/// guarantee holds and no session is ever created under keeper's cwd. Pure so
/// the invariant is unit-tested without a registry/tempdir.
fn resolve_destination_dir(stored: Option<String>, data_dir: &Path) -> PathBuf {
    if let Some(dir) = stored {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return path;
        }
    }
    dirs::video_dir()
        .unwrap_or_else(|| data_dir.to_path_buf())
        .join("keeper")
}

/// Resolve the EFFECTIVE recording path template (Story 40.2, AD-65): the
/// persisted user choice when one exists and still parses, otherwise
/// [`DEFAULT_TEMPLATE`]. The sibling of [`effective_destination_dir`], and it
/// exists for the same reason — the UI always receives a concrete value, and
/// "unset vs default" never reaches the frontend.
fn effective_path_template(data_dir: &Path) -> Result<String, IpcError> {
    let stored =
        keeper_core::registry::get_recording_path_template(data_dir).map_err(to_ipc_error)?;
    Ok(resolve_path_template(stored))
}

/// Resolve the effective template from the (already-fetched) persisted value.
///
/// Only a template that still PARSES is honored. A stored one that does not is
/// not an error here: `import_config_file` writes every `config.json` key into
/// the settings table verbatim and validates nothing, so a hand-edited row can
/// hold anything at all — and a settings surface that refused to load because
/// of it would leave the user with no way to fix the very field that broke it.
/// Degrading to the documented default on READ is the rule the fps and codec
/// getters already follow. The write path is where a bad template is refused,
/// out loud, with its reason. Pure so the invariant is unit-tested without a
/// registry/tempdir.
fn resolve_path_template(stored: Option<String>) -> String {
    stored
        .filter(|raw| PathTemplate::parse(raw).is_ok())
        .unwrap_or_else(|| DEFAULT_TEMPLATE.to_owned())
}

/// The startup orphan-recovery pass (Story 17.3, FR-73, AD-37): derive the
/// current EFFECTIVE recordings destination (the same
/// [`effective_destination_dir`] source of truth `recording_start` uses) and
/// run the core `recover_orphaned_sessions` scan over it, marking every
/// crash-orphaned `recording` manifest `recovered` on disk — the durable
/// signal Story 20.3's notice consumes. Best-effort end to end: any failure is
/// logged and swallowed, never fatal (this runs on a detached boot thread —
/// see `lib.rs` `setup`).
///
/// Safe against a concurrent `recording_start` (the detached thread can still
/// be walking a slow volume after the user clicked Record): the scan holds the
/// [`AppState::recovery_scan`] mutex (serializing it against the pre-record
/// pass) and passes the SAME `is_active` predicate over
/// [`AppState::reserved_recording_folders`], so a folder a starting/live
/// session has reserved is skipped untouched — a live session's manifest is
/// never rewritten to `recovered` mid-capture.
pub(crate) fn recover_orphaned_recordings(state: &AppState) {
    let data_dir = match state.platform.data_dir() {
        Ok(data_dir) => data_dir,
        Err(error) => {
            tracing::warn!(%error, "startup recovery: could not resolve the data dir (non-fatal)");
            return;
        }
    };
    let destination = effective_recording_destination(&data_dir, &|need| {
        destination_profile_table(&state.platform, need)
    });
    let base = destination.root.clone();
    let _scan = plain_lock(&state.recovery_scan);
    let is_active = |folder: &Path| plain_lock(&state.reserved_recording_folders).contains(folder);
    let recovered = recover_orphaned_sessions(&base, &is_active);
    if !recovered.is_empty() {
        tracing::info!(
            count = recovered.len(),
            "startup recovery marked orphaned session(s) recovered"
        );
    }
    // Story 42.1: the same pass, for the index. The recovery walk above has just
    // reconciled every manifest under this root, so this is the moment those
    // manifests are most worth replaying into rows — and startup is the only
    // moment nothing is recording, so a walk that rewrites rows cannot race a
    // session that is appending to them.
    //
    // Sent, not called: the writer owns the one connection to `archive.db`, and
    // a rebuild is exactly the operation that must not open a second. Nothing
    // waits on it — a boot that cannot index is a boot whose folders are still
    // the truth.
    if let Some(archive) = state.accounts.archive() {
        archive.rebuild_recordings(
            base,
            match destination.kind {
                RecordingDestinationKind::Folder => "folder".to_owned(),
                RecordingDestinationKind::Profile => "profile".to_owned(),
            },
            destination.profile_id.clone(),
        );
    }
}

/// Map a loaded [`SessionManifest`] to the read-only [`RecordingSummaryVm`] the
/// completion / recovery cards render (Story 20.3): the session folder path plus
/// the manifest-authoritative screen-segment count and total on-disk bytes.
#[cfg(desktop)]
fn manifest_summary(folder: &Path, manifest: &SessionManifest) -> RecordingSummaryVm {
    RecordingSummaryVm {
        session_folder: folder.to_string_lossy().into_owned(),
        screen_segment_count: manifest.screen_segment_count(),
        total_bytes: manifest.total_bytes(),
        // Story 21.5: surface the user title (when set) to the completion card
        // and the recovery notice.
        title: manifest.meta.as_ref().and_then(|m| m.title.clone()),
    }
}

/// Summarize one session folder for the completion / in-app-recovered card
/// (Story 20.3, FR-71): load `folder/manifest.json` and return the
/// manifest-authoritative `{screenSegmentCount, totalBytes, sessionFolder}` — the
/// honest "Saved N segments · {size}" figures, never the live `segments_closed`
/// rotation counter. A manifest load failure surfaces as an [`IpcError`] so the
/// card can still fall back to folder + Reveal (the frontend omits count/size).
///
/// `async` per AD-34-5: reading and parsing a manifest off a slow (possibly
/// removable) volume must not hold the main thread.
#[cfg(desktop)]
#[tauri::command]
pub async fn recording_session_summary(folder: String) -> Result<RecordingSummaryVm, IpcError> {
    off_async_runtime(move || -> Result<RecordingSummaryVm, IpcError> {
        let path = PathBuf::from(folder);
        let manifest = SessionManifest::load(&path).map_err(|e| to_ipc_error(e.into()))?;
        Ok(manifest_summary(&path, &manifest))
    })
    .await?
}

/// Mobile stub for [`recording_session_summary`] (Story 20.3): recording is a
/// desktop-only surface — an honest `Unsupported` (`retriable: false`).
#[cfg(not(desktop))]
#[tauri::command]
pub async fn recording_session_summary(folder: String) -> Result<RecordingSummaryVm, IpcError> {
    let _ = folder;
    Err(to_ipc_error(CoreError::Unsupported(
        "recording session summaries are desktop-only".to_owned(),
    )))
}

// ---------------------------------------------------------------------------
// The recording note stub (Story 42.4, FR-142)
// ---------------------------------------------------------------------------

/// The vault subtree a session's note stub is written into.
///
/// It cannot be the recordings folder, even when one profile holds both:
/// `RecordingsConfig::validate` refuses a recordings root that overlaps the
/// vault, so "written through the notes writer" necessarily means a SIBLING
/// subtree of the vault, and this is it. That refusal is also what makes the
/// join safe without a second containment check — the two roots provably do not
/// nest.
#[cfg(desktop)]
const RECORDING_NOTES_DIR: &str = "recordings";

/// How much of a candidate note is read to decide whether it is a session's
/// stub. Frontmatter is the first thing in a file and keeper's own block is a
/// few hundred bytes, so a note whose block does not fit in this is — by that
/// very fact — not one keeper composed. The cap matters because the vault
/// subtree is a real directory a user may put real notes in, and identifying one
/// stub must not mean reading all of them.
#[cfg(desktop)]
const STUB_HEAD_BYTES: u64 = 8 * 1024;

/// Where one session's stub lives, and what its paths are measured against.
#[cfg(desktop)]
struct StubDestination {
    /// The directory holding the stub. Read for the taken-name set, and scanned
    /// to find an existing stub.
    dir: PathBuf,
    /// What every relative path in and about the stub is relative to.
    ///
    /// The synced folder for a vault destination, the session folder's parent
    /// otherwise. FR-145's anchor in both cases: the widest directory that gets
    /// cloned to the other machine as one unit, so a note that points at its
    /// recording still points at it there.
    anchor: PathBuf,
    /// The vault to write through, when the destination is one. `None` is not a
    /// degrade — it is the ordinary plain-folder destination.
    vault: Option<crate::notes_vault::Vault>,
}

/// Resolve where a session's stub goes.
///
/// Split from the registry lookup ([`session_vault`]) so the DECISION is a pure
/// function of two values a test can hand it. The registry and the sync engine
/// are process-wide singletons; a destination rule that could only be exercised
/// through them would be a rule nothing ever checked.
#[cfg(desktop)]
fn stub_destination(beside: &Path, vault: Option<crate::notes_vault::Vault>) -> StubDestination {
    match vault {
        Some(vault) => StubDestination {
            dir: vault.root.join(RECORDING_NOTES_DIR),
            anchor: vault.local_path.clone(),
            vault: Some(vault),
        },
        None => StubDestination {
            dir: beside.to_path_buf(),
            anchor: beside.to_path_buf(),
            vault: None,
        },
    }
}

/// The indexed vault for a destination profile, if it is one.
///
/// The degrade the spec names is the middle case, and it is the reason this is
/// not a bare registry lookup: a profile that IS flagged as a vault but has no
/// indexed vault (the registry is rebuilt when the profile set changes, and a
/// finalize can land before that) is a vault destination that did not resolve.
/// That is logged and the stub goes beside the session folder — which is the
/// spec's instruction, and the opposite of guessing at a vault root.
///
/// A profile that is simply not a vault is not a degrade and says nothing.
#[cfg(desktop)]
fn session_vault(profile_id: &str) -> Option<crate::notes_vault::Vault> {
    if let Some(vault) = crate::notes_vault::vault(profile_id) {
        return Some(vault);
    }
    let flagged = crate::sync::engine_if_open()
        .and_then(|engine| engine.list_profiles().ok())
        .is_some_and(|profiles| {
            profiles
                .iter()
                .any(|profile| profile.id == profile_id && profile.notes.is_some())
        });
    if flagged {
        tracing::warn!(
            profile = %profile_id,
            "recording note: this destination holds a notes vault but no indexed vault resolved \
             for it, so the stub is written beside the session folder instead"
        );
    }
    None
}

/// The enclosing sync profile's id for a session folder.
///
/// The commands have only a folder, where the sink has the session's own
/// destination profile. The two agree by construction: Story 41.2 refuses a
/// plain-folder destination that sits inside a synced profile's tree, so a
/// session folder is inside a profile's tree exactly when that profile was its
/// destination.
#[cfg(desktop)]
fn stub_profile_id(folder: &Path) -> Option<String> {
    let engine = crate::sync::engine_if_open()?;
    crate::sync::profile_for_path(&engine, folder)
        .ok()
        .flatten()
        .map(|profile| profile.id)
}

/// The session's immutable identity (Story 40.3), or `None` for a manifest that
/// predates it.
///
/// No identity means no stub. A `session:` link that was a guess — a path, or an
/// id derived from one — would break at the first retitle, and the note would
/// then point at nothing while looking like it pointed at something.
#[cfg(desktop)]
fn stub_session_id(manifest: &SessionManifest) -> Option<&str> {
    manifest.meta.as_ref()?.session_id.as_deref()
}

/// An RFC 3339 stamp as epoch milliseconds.
///
/// The composer needs both this and the string it came from: the string carries
/// the local calendar the note's date must be right about, this carries the
/// absolute instant its duration must be measured from. Parsed here because
/// `keeper-core` has no calendar library and is not acquiring one (AD-55).
#[cfg(desktop)]
fn stub_epoch_ms(stamp: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|at| at.timestamp_millis())
}

/// The session's files, each relative to the destination root — what the
/// stub's `files:` list carries, in the same frame as its `recording:` folder.
///
/// **The manifest's ledger order, never a sort.** That is the order the session
/// recorded in, and a sorted list would lift `camera-0000.mov` above the screen
/// segment it belongs beside.
///
/// `manifest.json` is in the list because the session really has one: it is the
/// file [`SessionManifest::write`] renames its temp over
/// (`<folder>/manifest.json`, FR-146 — written once at finalize), and the
/// terminal reconcile has done exactly that before [`write_recording_note_stub`]
/// composes anything. Nothing else in the folder is named here, because nothing
/// else is a file this path *knows* is there rather than guesses at.
///
/// A file whose path will not express itself relative to the anchor is dropped
/// rather than written absolute: FR-145 admits no exception, and a note that is
/// silent about one file is better than a note that is wrong on every machine
/// but this one.
#[cfg(desktop)]
fn stub_files(manifest: &SessionManifest, dest: &StubDestination) -> Vec<String> {
    let folder = manifest.folder();
    manifest
        .segments
        .iter()
        .map(|segment| segment.file.trim())
        .filter(|file| !file.is_empty())
        .chain(std::iter::once("manifest.json"))
        .filter_map(|file| relative_session_path(&dest.anchor, &folder.join(file)))
        .collect()
}

/// Compose one session's stub from its manifest. Pure but for reading the
/// manifest it is handed — every byte of IO is in this module's callers.
#[cfg(desktop)]
fn compose_stub(
    manifest: &SessionManifest,
    dest: &StubDestination,
    taken: &[String],
) -> Option<NoteStub> {
    let meta = manifest.meta.as_ref()?;
    let session_id = meta.session_id.as_deref()?;
    let relative_folder = relative_session_path(&dest.anchor, manifest.folder());
    let file_paths = stub_files(manifest, dest);
    let files: Vec<&str> = file_paths.iter().map(String::as_str).collect();
    Some(recording_note::compose(
        &SessionFacts {
            session_id,
            title: meta.title.as_deref(),
            started_at: manifest.started_at.as_deref(),
            ended_at: manifest.ended_at.as_deref(),
            started_ms: manifest.started_at.as_deref().and_then(stub_epoch_ms),
            ended_ms: manifest.ended_at.as_deref().and_then(stub_epoch_ms),
            participants: meta.participants.as_deref(),
            tags: meta.tags.as_deref().unwrap_or(&[]),
            relative_folder: relative_folder.as_deref(),
            files: &files,
        },
        taken,
    ))
}

/// The file names already in the stub's directory — the set
/// [`recording_note::compose`] picks a free name against.
///
/// **This directory read is the whole of AC5.** Two sessions stopped in the same
/// minute share a minute-resolution stamp, so a stamp is not a name; only what
/// is actually on disk can say which names are gone. Inside a vault this is
/// `notes_vault::siblings`, the same helper `create_note` uses, so a stub and a
/// hand-made note cannot disagree about whether a name is free.
#[cfg(desktop)]
fn stub_taken_names(dest: &StubDestination) -> Vec<String> {
    match dest.vault.as_ref() {
        Some(vault) => crate::notes_vault::siblings(vault, RECORDING_NOTES_DIR),
        None => std::fs::read_dir(&dest.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The head of a file, capped at [`STUB_HEAD_BYTES`]. Lossy, because a cap can
/// land mid-character and the only question being asked of these bytes is what
/// the `session:` field says.
#[cfg(desktop)]
fn stub_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(STUB_HEAD_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    Some(String::from_utf8_lossy(&head).into_owned())
}

/// The stub for `session_id` in one directory, found by reading the FRONTMATTER
/// rather than by guessing the filename.
///
/// The filename carries a collision counter, so it is not derivable — but the
/// `session:` field is exact, and it is the same field a retitle leaves alone.
/// Sorted before choosing, so the impossible case of two files claiming one
/// session resolves the same way on every run instead of following `read_dir`.
#[cfg(desktop)]
fn find_stub(dir: &Path, session_id: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("md"))
        .filter(|path| {
            stub_head(path).is_some_and(|head| {
                Frontmatter::parse(&head).0.as_string("session") == Some(session_id)
            })
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// [`find_stub`] over both places a stub can be.
///
/// The resolved destination first, then beside the session folder. The second
/// look is not redundancy for its own sake: a profile that gains or loses its
/// notes flag between the finalize that wrote the stub and the card that shows
/// it would otherwise orphan a real file the user can no longer dismiss.
#[cfg(desktop)]
fn locate_stub(folder: &Path, dest: &StubDestination, session_id: &str) -> Option<PathBuf> {
    if let Some(found) = find_stub(&dest.dir, session_id) {
        return Some(found);
    }
    let beside = folder.parent()?;
    if beside == dest.dir {
        return None;
    }
    find_stub(beside, session_id)
}

/// Write the stub's bytes, through the notes writer when it lands in a vault.
///
/// Through `notes_vault::write_note` is what makes the vault case appear in the
/// notes index — the containment check, the parent creation and the atomic
/// replace all come with it. Outside a vault there is no writer to reach and no
/// index to appear in, so this is a plain write to a plain folder.
///
/// Both halves fail as a [`NotesError`], so a save that could not land reaches
/// the surface through the same funnel as every other note write in the app
/// rather than through a second mapping invented here. The message names the
/// FILE, never the path: this also runs on the finalize path, where everything
/// is logged.
#[cfg(desktop)]
fn write_stub(dest: &StubDestination, path: &Path, contents: &str) -> Result<(), NotesError> {
    let in_vault = dest.vault.as_ref().and_then(|vault| {
        let rel = relative_session_path(&vault.root, path)?;
        Some((vault, rel))
    });
    if let Some((vault, rel)) = in_vault {
        return crate::notes_vault::write_note(vault, &rel, contents);
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| NotesError::Name(format!("{name}: {error}")))?;
    }
    std::fs::write(path, contents).map_err(|error| NotesError::Name(format!("{name}: {error}")))
}

/// "This recording has no identity, so there is no note to file against it."
///
/// Shared by the two commands that must refuse rather than shrug: a save with
/// nowhere to go loses the words, so it is an error, and the same sentence
/// covers both places the identity can turn out to be missing.
#[cfg(desktop)]
fn no_session_identity() -> IpcError {
    to_ipc_error(CoreError::Unsupported(
        "this recording has no session identity, so a note cannot be filed against it".to_owned(),
    ))
}

/// Project a stub on disk into the VM the stop surface renders.
///
/// `contents` is what the FILE holds, not what the composer would produce, so a
/// stub the user has already saved comes back as they left it and a re-seeded
/// draft can never resurrect text they deleted.
#[cfg(desktop)]
fn note_stub_vm(
    path: &Path,
    dest: &StubDestination,
    session_id: &str,
    contents: String,
) -> RecordingNoteStubVm {
    let (_, block_end) = Frontmatter::parse(&contents);
    // The blank separator line belongs to the block, not to the prose — the same
    // `+ 1` `create_note` applies to its caret hint. CRLF costs two, and a file
    // rewritten without a separator (or without frontmatter at all) costs none,
    // so this is measured rather than assumed.
    let rest = &contents[block_end..];
    let body = block_end
        + if rest.starts_with("\r\n") {
            2
        } else if rest.starts_with('\n') {
            1
        } else {
            0
        };
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    RecordingNoteStubVm {
        // UTF-16 code units, because the surface slices `contents` at this index
        // with JavaScript string semantics. Converted here, once, so a non-ASCII
        // title cannot split the block in the wrong place.
        body_offset: contents[..body].encode_utf16().count() as u32,
        // Never absolute, and never a placeholder: the surface prints this when
        // a dismissal keeps the note. Falls back to the bare name for a stub
        // found outside the anchor, which is still a true thing to call it.
        relative_path: relative_session_path(&dest.anchor, path)
            .unwrap_or_else(|| filename.clone()),
        filename,
        contents,
        in_vault: dest.vault.is_some(),
        session_id: session_id.to_owned(),
        path: path.to_string_lossy().into_owned(),
    }
}

/// Compose and write one session's stub at finalize — **best-effort, always**.
///
/// Every failure below is a log line and a return. Finalize has already
/// succeeded by the time this runs: the segments are on disk, the manifest is
/// written, the row is completed. A note that could not be written is a note
/// that could not be written; turning it into a recording failure would tell the
/// user their recording is at risk when it is not, and the snapshot the
/// single-child start-guard keys off must never be moved by anything here.
///
/// A re-finalize leaves an existing stub alone. Not because a second write would
/// fail, but because it would silently replace whatever the user had typed into
/// the first — the one thing this story exists to capture.
#[cfg(desktop)]
fn write_recording_note_stub(manifest: &SessionManifest, dest: &StubDestination) {
    let Some(session_id) = stub_session_id(manifest) else {
        tracing::debug!(
            "recording note: this session carries no identity, so no stub is composed — a \
             `session:` link that was a guess would be worse than no note"
        );
        return;
    };
    if locate_stub(manifest.folder(), dest, session_id).is_some() {
        tracing::debug!(
            session = %session_id,
            "recording note: this session already has a stub, so a second is not written"
        );
        return;
    }
    let taken = stub_taken_names(dest);
    let Some(stub) = compose_stub(manifest, dest, &taken) else {
        return;
    };
    let path = dest.dir.join(&stub.filename);
    match write_stub(dest, &path, &stub.contents) {
        Ok(()) => tracing::info!(
            session = %session_id,
            in_vault = dest.vault.is_some(),
            "recording note: a note stub is waiting for this session"
        ),
        Err(error) => tracing::warn!(
            %error,
            session = %session_id,
            "recording note: the stub could not be written; the recording is finalized and \
             untouched"
        ),
    }
}

/// The finalize hook: resolve the destination, then write. Desktop-only because
/// a vault is a synced folder and iOS has neither.
#[cfg(desktop)]
fn note_stub_at_finalize(manifest: &SessionManifest, profile_id: Option<&str>) {
    let Some(beside) = manifest.folder().parent() else {
        tracing::warn!(
            "recording note: this session folder has no parent, so there is nowhere to put a note \
             beside it"
        );
        return;
    };
    let dest = stub_destination(beside, profile_id.and_then(session_vault));
    write_recording_note_stub(manifest, &dest);
}

/// iOS records nothing and syncs no folders, so there is no session to write a
/// note about.
#[cfg(not(desktop))]
fn note_stub_at_finalize(manifest: &SessionManifest, profile_id: Option<&str>) {
    let _ = (manifest, profile_id);
}

/// Everything the three stub commands resolve before they can do anything.
#[cfg(desktop)]
struct StubLookup {
    manifest: SessionManifest,
    dest: StubDestination,
    session_id: String,
}

/// Load a session folder and resolve where its stub would be. `None` when the
/// session has no identity — such a session never had a stub, so every command
/// answers "nothing here" rather than failing.
#[cfg(desktop)]
fn stub_lookup(folder: &Path) -> Result<Option<StubLookup>, IpcError> {
    let manifest = SessionManifest::load(folder).map_err(|e| to_ipc_error(e.into()))?;
    let Some(session_id) = stub_session_id(&manifest).map(str::to_owned) else {
        return Ok(None);
    };
    let Some(beside) = folder.parent() else {
        return Ok(None);
    };
    let dest = stub_destination(
        beside,
        stub_profile_id(folder).as_deref().and_then(session_vault),
    );
    Ok(Some(StubLookup {
        manifest,
        dest,
        session_id,
    }))
}

/// The note stub waiting for one session, or `None` when there is none (Story
/// 42.4).
///
/// `None` is an ordinary answer, never an error: a stub that could not be
/// written was logged at finalize, and a dismissed one is gone on purpose. The
/// stop surface renders nothing in either case and the summary card stays whole.
///
/// `async` per AD-34-5 — this reads a directory that may be on a slow or
/// removable volume.
#[cfg(desktop)]
#[tauri::command]
pub async fn recording_note_stub(folder: String) -> Result<Option<RecordingNoteStubVm>, IpcError> {
    off_async_runtime(move || -> Result<Option<RecordingNoteStubVm>, IpcError> {
        let folder = PathBuf::from(folder);
        let Some(lookup) = stub_lookup(&folder)? else {
            return Ok(None);
        };
        let Some(path) = locate_stub(&folder, &lookup.dest, &lookup.session_id) else {
            return Ok(None);
        };
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                tracing::warn!(
                    %error,
                    session = %lookup.session_id,
                    "recording note: the stub exists but could not be read"
                );
                return Ok(None);
            }
        };
        Ok(Some(note_stub_vm(
            &path,
            &lookup.dest,
            &lookup.session_id,
            contents,
        )))
    })
    .await?
}

/// Save what the user typed (Story 42.4).
///
/// **The one command here whose errors are surfaced.** The words are in a
/// textarea and nowhere else until this returns, so a swallowed failure would
/// lose them — the exact opposite of the story's point. The stop surface prints
/// the sentence and keeps the editor open.
///
/// A stub that has vanished under the surface is re-created rather than refused,
/// for the same reason: the user's writing lands, whatever happened to the file
/// keeper composed.
#[cfg(desktop)]
#[tauri::command]
pub async fn recording_note_stub_save(folder: String, contents: String) -> Result<(), IpcError> {
    off_async_runtime(move || -> Result<(), IpcError> {
        let folder = PathBuf::from(folder);
        let lookup = stub_lookup(&folder)?.ok_or_else(no_session_identity)?;
        let path = match locate_stub(&folder, &lookup.dest, &lookup.session_id) {
            Some(path) => path,
            None => {
                let taken = stub_taken_names(&lookup.dest);
                let stub = compose_stub(&lookup.manifest, &lookup.dest, &taken)
                    .ok_or_else(no_session_identity)?;
                lookup.dest.dir.join(stub.filename)
            }
        };
        write_stub(&lookup.dest, &path, &contents).map_err(|error| to_ipc_error(error.into()))
    })
    .await?
}

/// Delete one stub if — and only if — the user never touched it. `true` when
/// the file was deleted, `false` when it was kept.
///
/// # What authorises the deletion
///
/// One fact and nothing else: **the bytes on disk are byte-identical to the stub
/// keeper itself composes for this session, recomposed here from the manifest.**
/// Not a flag, not a hash the caller carries, not anything the frontend says —
/// the command above passes only a folder, so no argument anyone could get wrong
/// can widen what this deletes.
///
/// A hash stored alongside would be the obvious alternative, and it is worse in
/// the way that matters: it is a second record of the truth, and it goes stale
/// exactly when the file is edited outside keeper — which is the case where
/// deleting is unrecoverable. Recomposition has no such state. Its cost is that
/// a stub composed by an older keeper stops matching a newer one and becomes
/// undismissable; that is the safe direction, and a dismissal happens seconds
/// after finalize in the same build.
///
/// **Every uncertainty keeps the file.** Unreadable, absent frontmatter, a
/// manifest that no longer composes, a failed delete: all `false`. Deleting a
/// note somebody wrote is the one unrecoverable mistake available here, and an
/// empty note left behind is merely untidy.
///
/// # Why this unlinks rather than trashing (NFR-30)
///
/// Every other vault deletion goes through `notes_vault::trash_note`, because it
/// is removing bytes a person wrote. This one has proved, byte for byte, that
/// nobody wrote anything: the file is exactly what keeper emitted. Trashing it
/// would leave the empty note behind under another name, which is precisely the
/// litter dismissing exists to prevent, and AC3 says no file remains.
#[cfg(desktop)]
fn dismiss_stub(lookup: &StubLookup, path: &Path) -> bool {
    let Ok(on_disk) = std::fs::read_to_string(path) else {
        tracing::warn!(
            session = %lookup.session_id,
            "recording note: the stub could not be read, so it is kept — a file keeper cannot \
             see the contents of is never one it deletes"
        );
        return false;
    };
    // The name is irrelevant to the comparison, so the taken set is empty: only
    // the contents decide, and a collision counter never reaches them.
    let Some(composed) = compose_stub(&lookup.manifest, &lookup.dest, &[]) else {
        return false;
    };
    if on_disk != composed.contents {
        tracing::debug!(
            session = %lookup.session_id,
            "recording note: this stub is no longer what keeper composed, so it is kept"
        );
        return false;
    }
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(
                %error,
                session = %lookup.session_id,
                "recording note: the untouched stub could not be deleted, so it stays"
            );
            false
        }
    }
}

/// Dismiss the stub for one session (Story 42.4) — see [`dismiss_stub`] for what
/// makes a deletion safe. `false` whenever the file is kept, including when
/// there was never one to delete; the stop surface treats that as "close the
/// card", never as a failure.
#[cfg(desktop)]
#[tauri::command]
pub async fn recording_note_stub_dismiss(folder: String) -> Result<bool, IpcError> {
    off_async_runtime(move || -> Result<bool, IpcError> {
        let folder = PathBuf::from(folder);
        let Some(lookup) = stub_lookup(&folder)? else {
            return Ok(false);
        };
        let Some(path) = locate_stub(&folder, &lookup.dest, &lookup.session_id) else {
            return Ok(false);
        };
        Ok(dismiss_stub(&lookup, &path))
    })
    .await?
}

/// Mobile stubs for the Story 42.4 commands: recording is a desktop-only
/// surface, and so is the vault a note would land in.
#[cfg(not(desktop))]
#[tauri::command]
pub async fn recording_note_stub(folder: String) -> Result<Option<RecordingNoteStubVm>, IpcError> {
    let _ = folder;
    Ok(None)
}

#[cfg(not(desktop))]
#[tauri::command]
pub async fn recording_note_stub_save(folder: String, contents: String) -> Result<(), IpcError> {
    let _ = (folder, contents);
    Err(to_ipc_error(CoreError::Unsupported(
        "recording notes are desktop-only".to_owned(),
    )))
}

#[cfg(not(desktop))]
#[tauri::command]
pub async fn recording_note_stub_dismiss(folder: String) -> Result<bool, IpcError> {
    let _ = folder;
    Ok(false)
}

/// Retitle a finished session by MOVING its folder (Story 40.4, FR-129).
///
/// Since Story 40.3 the template names the session and `meta.sessionId` is the
/// handle, which is what makes this possible at all: the folder is re-rendered
/// from the SAME effective template against the session's OWN start instant with
/// the new title, `manifest.json`'s title and `session` label follow it, and the
/// identity is not touched. A blank (or absent) title clears the title and moves
/// the session back to the name an untitled session would have had.
///
/// Two refusals, both before anything moves: a session whose folder is in the
/// live-reservation set ([`recording_session_live_error`] — the driver and the
/// sidecar hold absolute paths into it), and a folder that is not under the
/// effective destination root, because a retitle moves a session WITHIN its root
/// and the rendered destination is only meaningful relative to that root.
///
/// Returns the summary of the NEW folder, so the card that asked for the retitle
/// can repaint from the answer instead of re-deriving the path it hoped for —
/// and the kept status snapshot follows the move too
/// ([`repoint_recording_slot_output`]), because that snapshot is what the
/// frontend re-adopts on every remount and a stale one would put the card back
/// on a folder that no longer exists.
#[cfg(desktop)]
#[tauri::command]
pub async fn recording_retitle(
    state: State<'_, AppState>,
    folder: String,
    title: Option<String>,
) -> Result<RecordingSummaryVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let platform = Arc::clone(&state.platform);
    let reserved = Arc::clone(&state.reserved_recording_folders);
    // `async` per AD-34-5, and the whole move goes to the blocking pool as one
    // unit: two registry reads, a manifest load, a directory create, a rename and
    // a manifest write, any of which can be on a slow removable volume.
    let source = PathBuf::from(folder);
    let moving = source.clone();
    let resolving = Arc::clone(&platform);
    let (manifest, destination, root) = off_async_runtime(
        move || -> Result<(SessionManifest, PathBuf, PathBuf), IpcError> {
            let root = effective_destination_dir(&data_dir, &resolving);
            let source = moving;
            // Not under the root ⇒ refused. The rendered destination is a path
            // relative to this root, so retitling a folder from elsewhere would
            // MOVE it into the recordings destination — a different operation
            // than the one the user asked for. `session_relative_key` is the same
            // reduction the recovery scan keys on, and it also rejects the root
            // itself, which is not a session either.
            if session_relative_key(&root, &source).is_none() {
                return Err(IpcError {
                    code: IpcErrorCode::Internal,
                    message: format!(
                        "\"{}\" is not inside the recordings destination, so it cannot be retitled",
                        source.display()
                    ),
                    account_id: None,
                    retriable: false,
                });
            }
            // The EFFECTIVE template, read now rather than remembered from the
            // start: a retitle names the session the way a start today would.
            // `effective_path_template` only ever returns one that parses, so the
            // refusal below is unreachable — and an honest error beats a panic if
            // that invariant ever breaks.
            let template_source = effective_path_template(&data_dir)?;
            let template = PathTemplate::parse(&template_source).map_err(|reason| {
                to_ipc_error(CoreError::Recording(RecordingError::TemplateInvalid {
                    reason,
                }))
            })?;
            let (manifest, destination) =
                retitle_session_folder(&reserved, &root, &template, &source, title.as_deref())?;
            Ok((manifest, destination, root))
        },
    )
    .await??;
    // The live-session refusal happens inside the move, so anything the slot
    // still names here is a FINISHED session — the terminal snapshot
    // `recording_stop` left behind for the summary card to render. Repoint it
    // only when it names the folder that actually moved; a slot describing some
    // other session is none of this retitle's business.
    if source != destination {
        repoint_recording_slot_output(&state.recording_run, &source, &destination);
    }
    // Story 42.1, matrix row 11: the row follows the session, so the move has to
    // reach the index too. Only the path is sent — `session_id` is the row's key
    // precisely so a retitle can move the folder without orphaning it, and the
    // codec and frame rate the row also carries exist in no manifest, so
    // rebuilding a whole row from what a retitle knows would write nulls over
    // them.
    //
    // Best-effort, and after the rename rather than around it: the folder has
    // already moved on disk, and the index is a cache of what the folders say.
    // A pre-Story-40.3 session, whose id is derived FROM its path
    // (`fallback_session_id`), mints a different id at the new location and so
    // updates nothing — the consequence that function documents, and a stale row
    // for a legacy session is a smaller wrong than refusing the rename.
    index_retitled_session(&state, &root, &destination, &manifest);
    // Detached, never awaited (Story 40.4). The leg below is a whole sync cycle
    // — commit, pull, LFS drain, push — and awaiting it would make Save sit out
    // a network timeout against an unreachable remote for a rename that has
    // ALREADY succeeded on disk and is the answer being returned on the very
    // next line. The spec's "profile paused or offline ⇒ the rename still
    // succeeded locally and is returned" row is exactly this.
    let moved = destination.clone();
    tauri::async_runtime::spawn(async move { sync_retitled_session(platform, moved).await });
    Ok(manifest_summary(&destination, &manifest))
}

/// A finished session's `meta` block, for the two surfaces that open a form on
/// it (Story 45.19, FR-197): the editor on the last recording, and "record
/// another like this" on a recording's note.
///
/// `Ok(None)` — never an error — for a folder with no loadable `manifest.json`.
/// A session whose manifest is missing or unparseable is one keeper can say
/// nothing about, and the two callers want the same thing from that: the editor
/// stays shut and the duplicate action is absent, rather than either of them
/// offering a form that would save into nothing. A load failure is not
/// actionable by the person reading the card — the recording itself is fine —
/// so it is logged and not raised.
///
/// `async` per AD-34-5: the manifest may be on a slow removable volume.
#[cfg(desktop)]
#[tauri::command]
pub async fn recording_session_meta(
    folder: String,
) -> Result<Option<RecordingSessionMetaVm>, IpcError> {
    off_async_runtime(move || -> Option<RecordingSessionMetaVm> {
        let path = PathBuf::from(folder);
        match SessionManifest::load(&path) {
            Ok(manifest) => Some(manifest.meta.unwrap_or_default().to_form_vm()),
            Err(error) => {
                tracing::info!(
                    %error,
                    "recording meta: no loadable manifest, so the session's details cannot be shown"
                );
                None
            }
        }
    })
    .await
}

/// Rewrite a finished session's metadata from the "Next session" form (Story
/// 45.19, FR-197) — every field of it EXCEPT the title.
///
/// **The title is [`recording_retitle`]'s and stays there.** Setting one MOVES
/// the session: Story 40.4 re-renders the path template against the session's
/// own start instant, renames the folder, repoints the kept status snapshot and
/// the archive row, and refuses a live session by name. Absolutely none of that
/// is true of participants, note, tags or custom rows, which are a rewrite of
/// four keys in a file. One editor collects both and sends each field to the one
/// command that owns it, so neither becomes a second answer to the other.
///
/// Refused for a live session ([`recording_session_live_error`]) by the same
/// compare-and-set claim the retitle uses, and for the same reason: the driver
/// and the sidecar hold this manifest open, and a rewrite under them would be
/// overwritten at the next reconcile at best. A folder with no loadable manifest
/// is refused too — there is nothing to edit, and creating one here would invent
/// a session out of a directory.
///
/// **The archive row is deliberately not rewritten**, exactly as a retitle does
/// not rewrite it: Story 42.1's row carries a codec and a frame rate that exist
/// in no manifest, so rebuilding one from what this edit knows would write nulls
/// over them, and the row is keyed on the identity this edit never touches. The
/// consequence is honest and bounded — the recordings browser keeps searching
/// the tags the session was STARTED with until the index is rebuilt from disk
/// ([`keeper_core::archive::recordings::rebuild_from_disk`]), which reads the
/// manifests.
#[cfg(desktop)]
#[tauri::command]
pub async fn recording_meta_update(
    state: State<'_, AppState>,
    folder: String,
    participants: Option<String>,
    note: Option<String>,
    tags: Option<String>,
    custom: Option<Vec<keeper_core::recording::SessionMetaField>>,
) -> Result<RecordingSessionMetaVm, IpcError> {
    let reserved = Arc::clone(&state.reserved_recording_folders);
    // The whole edit goes to the blocking pool as one unit (AD-34-5): a manifest
    // load and an atomic rewrite, either of which can be on a slow volume.
    off_async_runtime(move || -> Result<RecordingSessionMetaVm, IpcError> {
        let path = PathBuf::from(folder);
        // The claim IS the live check, as in `retitle_session_folder`: `reserve`
        // reports whether THIS guard inserted the entry, so a folder a live (or
        // starting) session holds is refused as one indivisible compare-and-set
        // rather than a `contains` a start could win the instant after it read
        // `false`. Held across the load and the write, so the orphan-recovery
        // pass cannot reconcile and rewrite this manifest from under the edit.
        let claim = LiveFolderReservation::reserve(&reserved, path.clone());
        if !claim.owned {
            return Err(recording_session_live_error());
        }
        let mut manifest = SessionManifest::load(&path).map_err(|err| to_ipc_error(err.into()))?;
        manifest.edit_details(&keeper_core::recording::SessionMetaInput {
            // Not sent, and not defaulted from the form either: `edit_details`
            // carries the manifest's own title through untouched.
            title: None,
            participants: participants.as_deref(),
            note: note.as_deref(),
            tags: tags.as_deref(),
            custom: custom.as_deref().unwrap_or(&[]),
        });
        manifest.write().map_err(|err| to_ipc_error(err.into()))?;
        // Answered from the manifest that was just written, not echoed back from
        // the request: the editor repaints from this, and the two differ wherever
        // a rule applied (a trimmed field, a dropped nameless row, a tag line
        // re-joined from its tokens). Echoing the request would show the user
        // their own typing and hide what was actually stored.
        Ok(manifest.meta.unwrap_or_default().to_form_vm())
    })
    .await?
}

/// Mobile stubs for the Story 45.19 commands: recording is a desktop-only
/// surface, so there is never a session manifest to read or rewrite.
#[cfg(not(desktop))]
#[tauri::command]
pub async fn recording_session_meta(
    folder: String,
) -> Result<Option<RecordingSessionMetaVm>, IpcError> {
    let _ = folder;
    Ok(None)
}

#[cfg(not(desktop))]
#[tauri::command]
pub async fn recording_meta_update(
    folder: String,
    participants: Option<String>,
    note: Option<String>,
    tags: Option<String>,
    custom: Option<Vec<keeper_core::recording::SessionMetaField>>,
) -> Result<RecordingSessionMetaVm, IpcError> {
    let _ = (folder, participants, note, tags, custom);
    Err(to_ipc_error(CoreError::Unsupported(
        "recording metadata is desktop-only".to_owned(),
    )))
}

/// How deep below a retitled session folder the prime walk descends.
///
/// A session folder holds its segments and its `manifest.json`, and any nesting
/// is the recorder's own — one or two levels at most. The cap only has to stop a
/// pathological tree (a user-planted symlink farm is already excluded by the
/// symlink skip) from turning a rename into an unbounded walk.
#[cfg(desktop)]
const RETITLE_PRIME_MAX_DEPTH: usize = 8;

/// How many entries the prime walk visits before it stops.
///
/// An hour of rotated segments is in the hundreds, so this is far above any real
/// session; tripping it is logged, and the only consequence is that the files
/// past the budget serve an ordinary settle window and the move splits into two
/// commits — the same outcome as no priming at all.
#[cfg(desktop)]
const RETITLE_PRIME_MAX_VISITS: usize = 50_000;

/// Every regular file under a just-moved session folder, absolute.
///
/// Iterative over an explicit stack, bounded on depth and on visits, and
/// symlink-skipping: a symlinked entry can point outside the session (or at its
/// own ancestor), and declaring a path settled is a claim about bytes this move
/// actually carried. Total and best-effort — an unreadable directory or entry is
/// logged at debug and skipped, because a file that is missed is only a file
/// that settles the slow way.
#[cfg(desktop)]
fn moved_session_files(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut pending: Vec<(PathBuf, usize)> = vec![(folder.to_path_buf(), 0)];
    let mut visits = 0usize;
    'walk: while let Some((directory, depth)) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::debug!(%error, "retitle: a moved directory could not be listed for priming");
                continue;
            }
        };
        for entry in entries {
            if visits == RETITLE_PRIME_MAX_VISITS {
                tracing::warn!(
                    budget = RETITLE_PRIME_MAX_VISITS,
                    "retitle: stopping the prime walk at its visit budget; the rest of the move may commit separately"
                );
                break 'walk;
            }
            visits += 1;
            let Ok(entry) = entry else { continue };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if depth < RETITLE_PRIME_MAX_DEPTH {
                    pending.push((entry.path(), depth + 1));
                }
                continue;
            }
            files.push(entry.path());
        }
    }
    files
}

/// Hand a retitled session's move to sync as ONE commit (Story 40.4) —
/// best-effort, and never able to fail the rename that already happened.
///
/// **Why sync is poked here at all.** git records no rename metadata; it infers
/// one at diff time from content, which is all `git log --follow` needs — but
/// only if the disappearance and the arrival are in the SAME commit. Left to the
/// watcher they would not be: `collect_stable_changes` admits a deletion at once
/// (there is no file left to sample) while every moved file arrives at an
/// absolute path the stability gate has never observed — and an unobserved path
/// is `Settling` by construction, because one sample is not two. The next tick
/// would commit the delete alone, the add would land a window later, and
/// `--follow` stops at the split.
///
/// **Why priming, and not merely syncing now.** Triggering a sync is not enough
/// on its own for exactly that reason: a single pass would still hold every
/// arrival. So the destination's files are declared already-settled first
/// ([`keeper_sync::Engine::prime_moved_paths`]) — which is the truth, since a
/// renamed file has been quiet for as long as it existed under its old name —
/// and only then is the sync triggered. One pass, both halves, one commit.
///
/// **The one gap this does not close.** A supervisor tick that walks the tree
/// in the window between the rename and the prime still sees a bare deletion,
/// and commits it alone. Nothing here can prevent that — the engine decides
/// when to walk (`Engine::scan_due`: a watcher wake, an elapsed settle window,
/// or the paced backstop), and by the time this code runs the rename has
/// already happened. The window is the walk plus one `sync.db` transaction —
/// single-digit milliseconds for a session folder — against a watcher whose
/// debouncer holds a rename event for `DEFAULT_DEBOUNCE_MS` (500 ms) before a
/// 1 Hz tick can act on it, so losing the race needs a volume slow enough, or a
/// session large enough, to spend half a second listing a directory. Closing it
/// outright would take an engine-side barrier declared BEFORE the rename — a
/// per-profile hold that makes the walk skip the profile until the move is
/// announced complete — which is a change to the scan schedule, not to this
/// leg. Losing costs `--follow` across this one rename and nothing else.
///
/// **Why every failure is swallowed.** The rename has already succeeded locally
/// and has already been returned to the caller. No git, no engine, no profile, a
/// paused profile, an unreachable remote or a sync error are each logged and
/// dropped — a folder that was renamed must never be reported as a failure, and
/// the next scheduled sync still picks the change up (as two commits, which
/// costs `--follow` and nothing else).
#[cfg(desktop)]
async fn sync_retitled_session(platform: Arc<dyn Platform>, folder: PathBuf) {
    let engine = match crate::sync::engine(platform) {
        Ok(engine) => engine,
        Err(error) => {
            tracing::warn!(
                %error,
                "retitle: no sync engine, so a synced move commits on the next sync instead"
            );
            return;
        }
    };
    let profile = match crate::sync::profile_for_path(engine.as_ref(), &folder) {
        Ok(Some(profile)) => profile,
        Ok(None) => {
            tracing::debug!(
                "retitle: the session is not inside a synced folder; nothing to commit"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "retitle: the sync profiles could not be read");
            return;
        }
    };
    // The walk and the prime are both synchronous disk work (a `read_dir` per
    // level, an `lstat` per file, one short `sync.db` transaction), so they go
    // to the blocking pool rather than occupying a runtime worker on a slow
    // removable volume.
    let priming = {
        let engine = Arc::clone(&engine);
        let profile_id = profile.id.clone();
        tokio::task::spawn_blocking(move || {
            let files = moved_session_files(&folder);
            engine.prime_moved_paths(&profile_id, &files)
        })
        .await
    };
    match priming {
        Ok(Ok(primed)) => tracing::debug!(
            profile = %profile.id,
            primed,
            "retitle: the moved files are declared settled, so the move can commit as one change"
        ),
        // Both failures cost the single commit and nothing else, so the sync is
        // still worth running: two commits beat an uncommitted move.
        Ok(Err(error)) => tracing::warn!(
            %error,
            profile = %profile.id,
            "retitle: the moved files could not be primed; the move may commit as two commits"
        ),
        Err(error) => tracing::warn!(
            %error,
            profile = %profile.id,
            "retitle: the prime task failed; the move may commit as two commits"
        ),
    }
    match engine
        .sync_once(&profile.id, keeper_sync::provenance::SyncSource::Manual)
        .await
    {
        Ok(outcome) => tracing::info!(
            profile = %profile.id,
            committed = outcome.committed.is_some(),
            "retitle: the move was handed to sync as one change"
        ),
        Err(error) => tracing::warn!(
            %error,
            profile = %profile.id,
            "retitle: a later sync will commit the move (as two commits)"
        ),
    }
}

/// Mobile stub for [`recording_retitle`] (Story 40.4): recording is a
/// desktop-only surface — an honest `Unsupported` (`retriable: false`).
#[cfg(not(desktop))]
#[tauri::command]
pub async fn recording_retitle(
    state: State<'_, AppState>,
    folder: String,
    title: Option<String>,
) -> Result<RecordingSummaryVm, IpcError> {
    let _ = (state, folder, title);
    Err(to_ipc_error(CoreError::Unsupported(
        "retitling a recording session is desktop-only".to_owned(),
    )))
}

/// List the crash-recovered sessions that still need surfacing (Story 20.3,
/// FR-73): walk the effective recordings destination's descendants (Story 40.3 —
/// the template may nest) for a loadable `manifest.json` with `status ==
/// Recovered` whose acknowledgement key ([`session_acknowledgement_key`] — the
/// session's identity, or its root-relative path when it predates one) is NOT in
/// the persisted seen-set, map each to a [`RecordingSummaryVm`], and return them
/// in a deterministic (root-relative path) order — see
/// [`scan_recovered_sessions`] for the guards.
///
/// Disk is the single source of truth — this re-derives the recovered set from
/// the on-disk `recovered` manifests Story 17.3 wrote (it also catches orphans
/// from prior app runs that the in-memory recovery list never held). Best-effort
/// and total: a missing/unreadable destination dir → `[]`; a per-entry load /
/// non-directory / non-recovered / acknowledged entry is skipped (logged), never
/// aborting the scan or propagating an error.
#[cfg(desktop)]
#[tauri::command]
pub async fn recovered_sessions_list(
    state: State<'_, AppState>,
) -> Result<Vec<RecordingSummaryVm>, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let platform = Arc::clone(&state.platform);
    // `async` per AD-34-5, and the whole scan goes to the blocking pool as one
    // unit: two `keeper.db` reads plus a `read_dir` with a manifest load per
    // subfolder is the heaviest command the Recording pane issues.
    off_async_runtime(move || -> Result<Vec<RecordingSummaryVm>, IpcError> {
        let base = effective_destination_dir(&data_dir, &platform);
        let acknowledged = keeper_core::registry::get_recovered_sessions_acknowledged(&data_dir)
            .map_err(to_ipc_error)?;
        Ok(scan_recovered_sessions(&base, &acknowledged))
    })
    .await?
}

/// The sort key, and the acknowledgement key of a pre-40.3 session, for a
/// recovered session: `folder`'s path relative to the destination `root`,
/// `/`-joined the way a rendered [`RelativePath`] writes one, so the key reads
/// identically on every platform. `None` when `folder` is not under `root`,
/// when it IS `root`, when a component is not UTF-8, or when a component is
/// anything but a plain name — none of which can name a session the scan
/// produced.
///
/// **Why relative, not the basename** (Story 40.3): the default template nests,
/// so `<root>/2026/x` and `<root>/2027/x` share a basename and a basename key
/// would make them one entry — acknowledging either would silently suppress the
/// other. **Why this stays backward compatible**: a flat session's root-relative
/// path IS its basename, so every acknowledgement entry written before this
/// story keeps matching exactly the session it was written for.
///
/// **Why `..` is refused** (Story 40.4): `strip_prefix` is lexical and
/// `Path::components` preserves `ParentDir`, so `<root>/../../elsewhere/thing`
/// strips to a non-empty relative path and would pass a containment check built
/// on "is this `Some`". 40.4 is the first caller that acts DESTRUCTIVELY on the
/// answer — it renames the folder it was handed into the recordings root — and
/// "a retitle moves a session within its root, never between roots" cannot be
/// enforced by a guard three dots defeat. Requiring every component to be
/// `Normal` also drops a `.`, which would otherwise key the same session two
/// ways.
#[cfg(desktop)]
fn session_relative_key(root: &Path, folder: &Path) -> Option<String> {
    let relative = folder.strip_prefix(root).ok()?;
    let mut key = String::new();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return None;
        };
        let component = component.to_str()?;
        if !key.is_empty() {
            key.push('/');
        }
        key.push_str(component);
    }
    (!key.is_empty()).then_some(key)
}

/// The persisted acknowledgement key for a session whose manifest LOADED: its
/// immutable `meta.sessionId` when it has one (Story 40.3), else its
/// root-relative path.
///
/// **Why the id wins.** 40.3 mints the identity precisely because the folder
/// path stopped being stable: 40.4 retitles a session by MOVING its folder, and
/// a user can drag one into another subfolder today. A path-keyed dismissal is
/// orphaned by either — the card comes back with nothing to explain it. The id
/// is written once and never edited, so a dismissal keyed on it survives every
/// move within the destination root.
///
/// **Why the path is still the fallback, not a migration.** A session recorded
/// before this story has no `sessionId` at all, and a flat pre-40.3 session's
/// root-relative path IS its basename — so those sessions keep the exact key
/// the seen-set already holds for them, and every entry written before 40.3
/// keeps suppressing exactly what it was written for.
#[cfg(desktop)]
fn session_acknowledgement_key<'a>(relative: &'a str, manifest: &'a SessionManifest) -> &'a str {
    manifest
        .meta
        .as_ref()
        .and_then(|meta| meta.session_id.as_deref())
        .unwrap_or(relative)
}

/// The pure, best-effort recovery scan behind [`recovered_sessions_list`] (Story
/// 20.3; nested since Story 40.3): walk the DESCENDANTS of `base`, keep every
/// folder whose `manifest.json` loads as `status == Recovered` and whose
/// [`session_acknowledgement_key`] is not in `acknowledged`, map each to a
/// [`RecordingSummaryVm`], and return them sorted by root-relative path
/// ([`session_relative_key`]) — a `/`-joined string, so the order is the same on
/// every platform instead of following `read_dir`. Total and best-effort: a
/// missing/unreadable `base` is `[]`, and any per-entry failure is logged and
/// skipped, never aborting the scan. Extracted from the command so it is
/// unit-testable over a temp dir without an `AppState`/registry.
///
/// **Why a walk.** Story 40.3 made `recording_start` render the path template,
/// whose default nests a session under `{yyyy}/`; a scan of `base`'s immediate
/// children would surface nothing at all after a crash. The guards are
/// `keeper_core::recording::recover_orphaned_sessions`'s:
/// - depth is counted in components below `base` (an immediate child is 1) and
///   capped at [`RECOVERY_MAX_DEPTH`]: a directory at the cap is still a
///   candidate, but nothing below it is ever read;
/// - the whole walk is capped at [`RECOVERY_MAX_VISITS`] directories, so a root
///   that is wide rather than deep is bounded too;
/// - `DirEntry::file_type` decides — never `is_dir()`/metadata — so a symlink is
///   skipped rather than followed out of the destination tree;
/// - a name starting with `.` is skipped whole (`.Trash` and friends: never a
///   session, never worth descending);
/// - a directory whose `manifest.json` LOADS is the session: it is evaluated and
///   never descended into, so a stray manifest inside a session cannot produce a
///   second card. A `manifest.json` that does not load marks nothing — the
///   directory is descended into as usual, so one unparseable file dropped in an
///   intermediate folder cannot hide every session beneath it.
///
/// **What the two walks actually share** is the two constants above, imported
/// from `keeper_core::recording` so the pass that MARKS a session `recovered`
/// and this scan that SURFACES it can never disagree about how far they reach.
/// The CODE is still duplicated, deliberately: the core's walk mutates
/// (reconcile + rewrite, gated on an `is_active` predicate) and yields folders,
/// this one is read-only, filters on the seen-set and yields view models, and
/// the only common shape left — "list the candidate directories" — would be a
/// new public core symbol carrying no behaviour. So the guards are spelled out
/// here and checkable against the list above.
///
/// One guard genuinely diverges, and only one: this walk skips a directory whose
/// name is not UTF-8, because its seen-set key must be a `String` and a card
/// whose dismissal could never be stored is worse than no card. The core walk
/// has no such gate — it recovers such a folder happily.
#[cfg(desktop)]
fn scan_recovered_sessions(base: &Path, acknowledged: &[String]) -> Vec<RecordingSummaryVm> {
    scan_recovered_sessions_within(base, acknowledged, RECOVERY_MAX_VISITS)
}

/// [`scan_recovered_sessions`] with its visit budget as an argument, so the
/// budget's own behaviour is provable against a tree of a dozen directories
/// rather than one of [`RECOVERY_MAX_VISITS`] — the same split
/// `keeper_core::recording::recover_orphaned_sessions` makes. Every shipping
/// caller goes through the entry point above, which passes the real budget.
#[cfg(desktop)]
fn scan_recovered_sessions_within(
    base: &Path,
    acknowledged: &[String],
    max_visits: usize,
) -> Vec<RecordingSummaryVm> {
    // Iterative, over an explicit stack: the root is user-chosen, so how deep a
    // recursive walk would go is not a question worth having (the caps bound the
    // work either way). Each entry is a directory and its own depth below
    // `base`, which is 0.
    let mut pending: Vec<(PathBuf, usize)> = vec![(base.to_path_buf(), 0)];
    // Keyed by the root-relative path, so the result is ordered by that key
    // (deterministic across `read_dir` orders and platforms — nesting sorts by
    // parent then leaf) without a second sort pass. The key is unique by
    // construction: a walk visits each folder once. This is the SORT key only —
    // the acknowledgement key is `session_acknowledgement_key`, which prefers
    // the session id and is therefore not ordered by location.
    let mut found: BTreeMap<String, RecordingSummaryVm> = BTreeMap::new();
    // Directories examined so far, against `max_visits`. Depth bounds a deep
    // root; this bounds a wide one — the two guards are independent, and a
    // Photos library trips only this one.
    let mut visits = 0usize;
    'walk: while let Some((directory, depth)) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            // A missing base is "no recordings yet"; a missing subdirectory
            // vanished mid-walk. Neither deserves a warning.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                tracing::warn!(%error, "recovered-sessions scan: could not read a directory; skipping it");
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(%error, "recovered-sessions scan: skipping unreadable dir entry");
                    continue;
                }
            };
            // A `DirEntry` file type does not follow symlinks: a symlinked entry
            // can point outside the destination tree, which the recovery pass
            // refuses to rewrite — so this scan refuses to surface it.
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    tracing::warn!(%error, "recovered-sessions scan: skipping entry with no file type");
                    continue;
                }
            };
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            let file_name = entry.file_name();
            // A name the seen-set could not hold is a name this scan cannot
            // acknowledge — skip the subtree rather than surface a card whose
            // dismissal would never stick.
            let Some(name) = file_name.to_str() else {
                tracing::debug!("recovered-sessions scan: skipping a non-UTF-8 directory name");
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            // The budget is spent on directories, and spent BEFORE the manifest
            // probe: the probe is one of the `stat` calls it exists to bound.
            // Tripping it truncates the scan rather than failing it — the cards
            // already found still surface, and the warn names the budget so the
            // log says which number to raise.
            if visits == max_visits {
                tracing::warn!(
                    budget = max_visits,
                    found = found.len(),
                    "recovered-sessions scan: stopping the walk at its visit budget; listing what was found so far"
                );
                break 'walk;
            }
            visits += 1;
            let folder = entry.path();
            // A `manifest.json` only NOMINATES a session; the load decides. A
            // manifest that LOADS is the session — evaluated here, never walked
            // — and one that does not marks nothing, so the directory is walked
            // like any other. Treating the probe as the answer made a stray file
            // a lid: every real session beneath it stayed invisible.
            if folder.join("manifest.json").is_file() {
                match SessionManifest::load(&folder) {
                    Ok(manifest) => {
                        if let Some(relative) = session_relative_key(base, &folder) {
                            let key = session_acknowledgement_key(&relative, &manifest);
                            if manifest.status == ManifestStatus::Recovered
                                && !acknowledged.iter().any(|entry| entry == key)
                            {
                                found.insert(relative, manifest_summary(&folder, &manifest));
                            }
                        }
                        continue;
                    }
                    Err(error) => {
                        tracing::debug!(%error, "recovered-sessions scan: manifest did not load; walking the directory instead of treating it as a session");
                    }
                }
            }
            // Not a session: descend, unless its children would sit past the
            // depth cap.
            if depth + 1 < RECOVERY_MAX_DEPTH {
                pending.push((folder, depth + 1));
            }
        }
    }
    found.into_values().collect()
}

/// Mobile stub for [`recovered_sessions_list`] (Story 20.3): recording is a
/// desktop-only surface — an honest empty list (no recovery on mobile).
#[cfg(not(desktop))]
#[tauri::command]
pub async fn recovered_sessions_list(
    state: State<'_, AppState>,
) -> Result<Vec<RecordingSummaryVm>, IpcError> {
    let _ = state;
    Ok(Vec::new())
}

/// Acknowledge (dismiss) a surfaced recovery card (Story 20.3, FR-73): latch the
/// session's acknowledgement key into the persisted seen-set so
/// [`recovered_sessions_list`] never surfaces it again on a later scan/restart.
/// One-way and idempotent.
///
/// The key is exactly the one [`scan_recovered_sessions`] compares
/// ([`session_acknowledgement_key`]): the manifest's immutable `meta.sessionId`
/// when it has one, else the folder's root-relative path
/// ([`session_relative_key`], reduced against the [`effective_destination_dir`]
/// root). Keying on the identity is what makes a dismissal survive Story 40.4's
/// retitle-by-move (and a user dragging a session elsewhere under the root),
/// which a path key would orphan — the card would return with nothing to
/// explain it. Pre-40.3 sessions have no id and fall back to the path; since a
/// flat session's root-relative path IS its basename, every entry written
/// before 40.3 still suppresses exactly the session it was written for.
///
/// **It cannot fail on the user.** The frontend drops the card from local state
/// the moment it calls this, so a rejection would show a successful dismiss and
/// then resurrect the card later with no explanation. Everything this needs to
/// READ is therefore best-effort: an unreadable destination setting, a `folder`
/// that is not under the destination root (or has no components to reduce), and
/// a manifest that will not load are each logged at `warn` and return `Ok(())`
/// without latching. Only the registry WRITE itself can return an error — a
/// genuine local-store failure, the one thing a retry could actually fix.
#[cfg(desktop)]
#[tauri::command]
pub fn recovered_session_acknowledge(
    state: State<'_, AppState>,
    folder: String,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    latch_recovered_session_acknowledgement(&data_dir, &state.platform, Path::new(&folder))
}

/// The registry half of [`recovered_session_acknowledge`], split out so the
/// "an environmental read failure is still a successful dismiss" rule is
/// testable over a temp data dir without Tauri state.
#[cfg(desktop)]
fn latch_recovered_session_acknowledgement(
    data_dir: &Path,
    platform: &Arc<dyn Platform>,
    folder: &Path,
) -> Result<(), IpcError> {
    let root = effective_destination_dir(data_dir, platform);
    let Some(relative) = session_relative_key(&root, folder) else {
        tracing::warn!(
            folder = %folder.display(),
            "acknowledge recovered session: folder is not under the destination root; skipping latch"
        );
        return Ok(());
    };
    // The manifest carries the key. Without it there is no honest key to store:
    // latching the path would be wrong for any session that HAS an id (the scan
    // would never compare it), and a folder whose manifest does not load is not
    // one the scan can surface anyway.
    let manifest = match SessionManifest::load(folder) {
        Ok(manifest) => manifest,
        Err(error) => {
            tracing::warn!(
                %error,
                folder = %folder.display(),
                "acknowledge recovered session: manifest did not load; skipping latch"
            );
            return Ok(());
        }
    };
    let key = session_acknowledgement_key(&relative, &manifest);
    keeper_core::registry::add_recovered_session_acknowledged(data_dir, key).map_err(to_ipc_error)
}

/// Mobile stub for [`recovered_session_acknowledge`] (Story 20.3): recording is a
/// desktop-only surface — an honest `Unsupported` (`retriable: false`).
#[cfg(not(desktop))]
#[tauri::command]
pub fn recovered_session_acknowledge(
    state: State<'_, AppState>,
    folder: String,
) -> Result<(), IpcError> {
    let _ = (state, folder);
    Err(to_ipc_error(CoreError::Unsupported(
        "recovered-session acknowledgement is desktop-only".to_owned(),
    )))
}

/// Probe whether the destination folder is actually writable (Story 19.5) with
/// a real probe-file write+remove — more reliable than metadata permissions on
/// macOS (ACLs, read-only volumes, sandboxing). The probe name is unique per
/// attempt (pid + nanos) so a crash mid-probe cannot leave a recognizable stray
/// file, and no two probes ever collide. Best-effort cleanup.
fn destination_writable(directory: &Path) -> bool {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let probe = directory.join(format!(
        ".keeper-write-probe-{}-{nanos}",
        std::process::id()
    ));
    let writable = std::fs::write(&probe, b"keeper").is_ok();
    let _ = std::fs::remove_file(&probe);
    writable
}

/// Read the effective recording settings from `keeper.db` (Story 17.5 + 19.5,
/// FR-72). All settings surfaces (Settings → Recording and the pre-record setup
/// cards) hydrate their shared store from this. The registry getters default
/// (500 MB / 30 min / 30 fps) and clamp/normalize defensively, and the
/// destination is resolved to the EFFECTIVE folder (the persisted choice or the
/// `~/Movies/keeper` default), so the VM always carries concrete, in-bounds
/// values. Failures funnel through [`to_ipc_error`].
///
/// `async` per AD-34-5: the Recording pane mounts three surfaces that each hydrate
/// from this, so six `keeper.db` reads plus a destination probe would otherwise
/// land on the main thread three times over on every visit to the tab.
#[tauri::command]
pub async fn recording_settings_get(
    state: State<'_, AppState>,
) -> Result<RecordingSettingsVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let platform = Arc::clone(&state.platform);
    off_async_runtime(move || {
        read_recording_settings(&data_dir, &|need| {
            destination_profile_table(&platform, need)
        })
    })
    .await?
}

/// Read the folder-card list sizes (folded / unfolded).
///
/// `async` for the same reason [`recording_settings_get`] is: the Sync pane
/// hydrates from this on every visit, and a `keeper.db` read belongs off the main
/// thread. The registry getters default and clamp, so the VM is always in bounds.
#[tauri::command]
pub async fn sync_list_settings_get(
    state: State<'_, AppState>,
) -> Result<SyncListSettingsVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    off_async_runtime(move || read_sync_list_settings(&data_dir)).await?
}

/// The blocking body of [`sync_list_settings_get`], shared with the setter's
/// re-read so there is one definition of "effective sizes" and the write path can
/// never return a pair the read path would not.
fn read_sync_list_settings(data_dir: &Path) -> Result<SyncListSettingsVm, IpcError> {
    Ok(SyncListSettingsVm {
        folded: keeper_core::registry::get_sync_list_folded(data_dir).map_err(to_ipc_error)?,
        unfolded: keeper_core::registry::get_sync_list_unfolded(data_dir).map_err(to_ipc_error)?,
    })
}

/// Persist the folder-card list sizes, returning what was actually stored.
///
/// Returns the re-read VM rather than the input: both values are clamped, so
/// echoing the request back would leave the UI displaying a number that is not in
/// the database.
#[tauri::command]
pub async fn sync_list_settings_set(
    state: State<'_, AppState>,
    settings: SyncListSettingsVm,
) -> Result<SyncListSettingsVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    off_async_runtime(move || -> Result<SyncListSettingsVm, IpcError> {
        keeper_core::registry::set_sync_list_folded(&data_dir, settings.folded)
            .map_err(to_ipc_error)?;
        keeper_core::registry::set_sync_list_unfolded(&data_dir, settings.unfolded)
            .map_err(to_ipc_error)?;
        read_sync_list_settings(&data_dir)
    })
    .await?
}

/// The blocking body of [`recording_settings_get`], shared with
/// [`recording_settings_set`]'s re-read so there is exactly one definition of
/// "effective settings" and the write path never returns a value the read path
/// would not.
///
/// `profiles` is the destination resolution's lazy view of this machine's synced
/// folders (Story 41.2) — consulted only when a profile id is actually stored,
/// and injected rather than derived from a platform so every degrade row is
/// asserted without an engine.
fn read_recording_settings(
    data_dir: &Path,
    profiles: &dyn Fn(ProfileTableNeed) -> DestinationProfileTable,
) -> Result<RecordingSettingsVm, IpcError> {
    let destination = effective_recording_destination(data_dir, profiles);
    Ok(RecordingSettingsVm {
        segment_mb: keeper_core::registry::get_recording_segment_mb(data_dir)
            .map_err(to_ipc_error)?,
        duration_cap_minutes: keeper_core::registry::get_recording_duration_cap_minutes(data_dir)
            .map_err(to_ipc_error)?,
        destination_dir: destination.root.to_string_lossy().into_owned(),
        destination_kind: destination.kind,
        destination_profile_id: destination.profile_id,
        destination_profile_name: destination.profile_name,
        destination_volume: destination.volume.as_ref().map(destination_volume_vm),
        fps: keeper_core::registry::get_recording_fps(data_dir).map_err(to_ipc_error)?,
        codec: keeper_core::registry::get_recording_codec(data_dir).map_err(to_ipc_error)?,
        scale_percent: keeper_core::registry::get_recording_scale_percent(data_dir)
            .map_err(to_ipc_error)?,
        echo_cancellation: keeper_core::registry::get_recording_echo_cancellation(data_dir)
            .map_err(to_ipc_error)?,
        path_template: effective_path_template(data_dir)?,
    })
}

/// Why a submitted recordings destination is refused, in the words the surface
/// prints (Story 41.2, UX-DR47).
///
/// One type so each sentence is written once and asserted in a test rather than
/// guessed at — the same reason [`echo_cancellation_locked_error`] is a named
/// function. Every message says what is wrong AND what to do next, and each of
/// the ones about a synced folder NAMES it: a refusal that will not say which
/// folder it collided with is the kind of message people file tickets about.
///
/// Built here rather than as a `keeper-core` `RecordingError` because a sync
/// profile is something only the shell knows about (AD-40 keeps the two crates
/// apart), and these sentences quote a profile's name.
enum DestinationRefusal {
    /// `kind: profile` with no id — a malformed submission, not a downgrade to
    /// the folder choice: silently recording somewhere the user did not choose is
    /// the whole failure mode this story exists to remove.
    NoProfileChosen,
    /// The id names no profile on this machine.
    UnknownProfile,
    /// The named profile is paused, so nothing recorded there would be committed.
    PausedProfile(String),
    /// The named profile does not say it holds recordings (Story 41.1's flag),
    /// which is a thing only `keeper-syncd` sets — never this surface.
    NotRecordingsProfile(String),
    /// There is no engine to verify the id against (no usable `git`).
    ProfilesUnreadable,
    /// A plain folder inside a synced folder's tree that is not its recordings
    /// root — the ambiguous case that would otherwise sync by accident.
    /// `offers_recordings` picks the next step that actually exists.
    InsideSyncedFolder {
        /// The synced folder the choice would have collided with.
        profile: String,
        /// Whether that folder can be chosen as the destination instead.
        offers_recordings: bool,
    },
    /// The chosen synced folder lives on removable media that is not attached
    /// (Story 41.7, AD-48). Raised at START, never at the settings write: the
    /// choice is perfectly good, the drive is simply in a drawer, and refusing to
    /// let someone choose their pendrive because it is unplugged right now would
    /// be a worse surface than the one this story is fixing.
    VolumeAbsent {
        /// The volume's own name — the actionable half of the sentence, and the
        /// whole reason `merope is not attached` beats an `EPERM` on a path.
        /// `None` when this run has never seen the drive and so cannot name it;
        /// the sentence then describes it instead of guessing at a name.
        volume: Option<String>,
        /// The synced folder that lives on it.
        profile: String,
    },
    /// Something is mounted where the chosen synced folder's volume lives, but it
    /// is not that volume (Story 41.7): a second stick at the same mountpoint, or
    /// a marker that would not read. `detail` is `keeper-sync`'s own account.
    VolumeUnexpected {
        /// The volume the profile expects, when this run has seen it before.
        volume: Option<String>,
        /// The synced folder that lives on it.
        profile: String,
        /// What was found instead.
        detail: String,
    },
}

impl DestinationRefusal {
    /// The sentence the settings surface renders verbatim beside the control.
    fn message(&self) -> String {
        match self {
            Self::NoProfileChosen => {
                "no synced folder was chosen — pick one, or record into a plain folder instead"
                    .to_owned()
            }
            Self::UnknownProfile => {
                "that synced folder is not set up on this machine — pick another destination"
                    .to_owned()
            }
            Self::PausedProfile(profile) => format!(
                "the synced folder \"{profile}\" is paused, so nothing recorded there would be committed — resume it, or pick another destination"
            ),
            Self::NotRecordingsProfile(profile) => format!(
                "the synced folder \"{profile}\" doesn't hold recordings — pick one that does, or record into a plain folder instead"
            ),
            Self::ProfilesUnreadable => {
                "the synced folders can't be read on this machine, so a synced destination can't be verified — record into a plain folder instead"
                    .to_owned()
            }
            Self::InsideSyncedFolder {
                profile,
                offers_recordings: true,
            } => format!(
                "that folder is inside the synced folder \"{profile}\", so recordings there would be committed by it without anything saying so — choose the synced folder \"{profile}\" itself, or a folder outside it"
            ),
            Self::InsideSyncedFolder {
                profile,
                offers_recordings: false,
            } => format!(
                "that folder is inside the synced folder \"{profile}\", so recordings there would be committed by it without anything saying so — choose a folder outside it, or let \"{profile}\" say it holds recordings"
            ),
            Self::VolumeAbsent {
                volume: Some(volume),
                profile,
            } => format!(
                "\"{volume}\" is not attached, so the synced folder \"{profile}\" is not on this machine right now — plug the drive in, or pick another destination. Nothing was recorded."
            ),
            Self::VolumeAbsent {
                volume: None,
                profile,
            } => format!(
                "the removable drive holding the synced folder \"{profile}\" is not attached — plug it in, or pick another destination. Nothing was recorded."
            ),
            Self::VolumeUnexpected {
                volume: Some(volume),
                profile,
                detail,
            } => format!(
                "the synced folder \"{profile}\" expects the volume \"{volume}\", but {detail} — check which drive is plugged in, or pick another destination. Nothing was recorded."
            ),
            Self::VolumeUnexpected {
                volume: None,
                profile,
                detail,
            } => format!(
                "the synced folder \"{profile}\" is not on the removable volume it expects: {detail} — check which drive is plugged in, or pick another destination. Nothing was recorded."
            ),
        }
    }

    /// Only "the synced folders could not be read" can succeed on a retry —
    /// installing a usable `git` changes that answer. Every other refusal is a
    /// statement about the submitted value, and resubmitting it fails the same way.
    ///
    /// The two volume refusals (Story 41.7) are deliberately in the second group,
    /// even though plugging the drive in does change the answer. `retriable` is
    /// read as "try the identical request again and it may work", and nothing
    /// keeper does can attach a drive: a retry with the stick still in a drawer
    /// fails identically, and inviting an automatic one would spin a loop around
    /// a human action. The message says what the human has to do instead.
    fn retriable(&self) -> bool {
        matches!(self, Self::ProfilesUnreadable)
    }

    fn into_error(self) -> IpcError {
        IpcError {
            code: IpcErrorCode::RecordingDestinationRefused,
            message: self.message(),
            account_id: None,
            retriable: self.retriable(),
        }
    }
}

/// A scanned volume as the settings VM carries it (Story 41.7).
///
/// The `Unexpected` detail stays shell-side: the card says "a different volume is
/// mounted where yours lives" and the REFUSAL carries `keeper-sync`'s specific
/// account, because a resting settings pane is not the place for a marker's parse
/// error but a refused Start absolutely is.
fn destination_volume_vm(volume: &DestinationVolume) -> RecordingVolumeVm {
    RecordingVolumeVm {
        name: volume.name.clone(),
        state: match volume.status {
            DestinationVolumeStatus::Attached => RecordingVolumeState::Attached,
            DestinationVolumeStatus::Absent => RecordingVolumeState::Absent,
            DestinationVolumeStatus::Unexpected { .. } => RecordingVolumeState::Unexpected,
        },
    }
}

/// Whether a resolved destination's volume forbids starting a recording (Story
/// 41.7) — `None` when it does not.
///
/// The counterpart to [`resolve_recording_destination`]'s deliberate silence: the
/// resolution stays total and keeps naming the folder the owner chose, and THIS
/// is where "that folder is not here right now" stops a Start. Called before
/// anything is created — before the pre-record recovery pass, which writes
/// manifests, and long before `create_dir_all` — so a refused Start leaves the
/// filesystem exactly as it found it.
///
/// A plain folder has no volume and can never be refused here, which keeps
/// Story 19.5's destination gate the only judge of an ordinary folder.
fn destination_volume_refusal(destination: &RecordingDestination) -> Option<IpcError> {
    let volume = destination.volume.as_ref()?;
    // A resolved profile destination always carries its name; the fallback keeps
    // the sentence from being built around an empty pair of quotes if it ever
    // does not.
    let profile = destination
        .profile_name
        .clone()
        .or_else(|| volume.name.clone())
        .unwrap_or_else(|| "this synced folder".to_owned());
    match &volume.status {
        DestinationVolumeStatus::Attached => None,
        DestinationVolumeStatus::Absent => Some(
            DestinationRefusal::VolumeAbsent {
                volume: volume.name.clone(),
                profile,
            }
            .into_error(),
        ),
        DestinationVolumeStatus::Unexpected { detail } => Some(
            DestinationRefusal::VolumeUnexpected {
                volume: volume.name.clone(),
                profile,
                detail: detail.clone(),
            }
            .into_error(),
        ),
    }
}

/// Which settings key a submitted destination becomes (Story 41.2).
///
/// Exactly one variant, because exactly one key is ever in force: writing either
/// clears the other, so the state the getter has to resolve profile-first cannot
/// be created by this command at all.
enum DestinationChoice {
    /// Store this folder under `recording.destination_dir` (blank CLEARS it, which
    /// reads back as the default root) and clear the profile key.
    Folder(String),
    /// Store this profile id under `recording.destination_profile_id` and clear
    /// the folder key.
    Profile(String),
}

/// Decide what a submitted destination becomes, or refuse it — BEFORE anything is
/// written (Story 41.2, FR-131, UX-DR47).
///
/// `destination_kind` is the discriminator: under `Profile` the folder field is an
/// answer this command produced rather than a question the user asked, and under
/// `Folder` the id is. The two refusals are the ambiguities the epic refuses to
/// let happen quietly — a profile that cannot hold recordings, and a folder that
/// would be committed by a synced folder with nothing anywhere saying so — plus
/// the one exception that is not ambiguous at all: a folder that IS a synced
/// folder's recordings root is the same place as that profile's choice, and only
/// one of the two carries the consequence, so it is normalised to the profile.
fn destination_choice(
    settings: &RecordingSettingsVm,
    profiles: &dyn Fn(ProfileTableNeed) -> DestinationProfileTable,
) -> Result<DestinationChoice, IpcError> {
    match settings.destination_kind {
        RecordingDestinationKind::Profile => {
            let id = settings
                .destination_profile_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| DestinationRefusal::NoProfileChosen.into_error())?;
            // Unverifiable is refused here, unlike on the read side: storing a
            // choice keeper cannot resolve would hand the user a destination that
            // silently is not the one they picked.
            let rows = profiles(ProfileTableNeed::Chosen).map_err(|reason| {
                tracing::warn!(
                    %reason,
                    "recording settings: a synced destination cannot be verified, so it is refused"
                );
                DestinationRefusal::ProfilesUnreadable.into_error()
            })?;
            let row = rows
                .iter()
                .find(|row| row.id == id)
                .ok_or_else(|| DestinationRefusal::UnknownProfile.into_error())?;
            if !row.enabled {
                return Err(DestinationRefusal::PausedProfile(row.name.clone()).into_error());
            }
            if row.recordings.is_none() {
                return Err(DestinationRefusal::NotRecordingsProfile(row.name.clone()).into_error());
            }
            Ok(DestinationChoice::Profile(id.to_owned()))
        }
        RecordingDestinationKind::Folder => {
            let submitted = settings.destination_dir.trim();
            // Blank is not a refusal: it clears the key and the effective read
            // returns the `~/Movies/keeper` default, which is both how a surface
            // says "back to the default folder" and how it leaves a profile.
            if submitted.is_empty() {
                return Ok(DestinationChoice::Folder(String::new()));
            }
            // A machine with no usable `git` has no synced folders to collide
            // with and must still be able to choose where it records (NFR-34), so
            // here the missing table skips the check out loud rather than
            // becoming a refusal.
            let rows = match profiles(ProfileTableNeed::Chosen) {
                Ok(rows) => rows,
                Err(reason) => {
                    tracing::warn!(
                        %reason,
                        "recording settings: the chosen folder could not be checked against the synced folders"
                    );
                    return Ok(DestinationChoice::Folder(submitted.to_owned()));
                }
            };
            let folder = Path::new(submitted);
            match enclosing_destination_profile(&rows, folder) {
                Some(row) if row.recordings_root() == Some(folder) => {
                    tracing::info!(
                        profile = %row.id,
                        "recording settings: the chosen folder IS a synced folder's recordings root, so it is stored as that choice"
                    );
                    Ok(DestinationChoice::Profile(row.id.clone()))
                }
                Some(row) => Err(DestinationRefusal::InsideSyncedFolder {
                    profile: row.name.clone(),
                    offers_recordings: row.recordings.is_some(),
                }
                .into_error()),
                None => Ok(DestinationChoice::Folder(submitted.to_owned())),
            }
        }
    }
}

/// The recordings-flagged synced folders this machine can record into (Story
/// 41.2, FR-131) — the destination picker's only data source.
///
/// Enabled AND flagged only, which is the same rule the resolution and the setter
/// use: a folder that has not said it holds recordings is not a destination, and
/// a paused one resolves to the plain folder anyway, so offering either would be
/// offering a choice keeper would then quietly ignore (AD-27's "no dead buttons").
///
/// An empty list is a normal answer and never an error: on a machine with no
/// usable `git`, no engine, or simply no flagged profile the surface renders
/// exactly today's single folder chooser — no empty picker, no new copy.
#[tauri::command]
pub async fn recording_destination_profiles(
    state: State<'_, AppState>,
) -> Result<Vec<RecordingProfileVm>, IpcError> {
    let platform = Arc::clone(&state.platform);
    // `async` per AD-34-5: this opens `sync.db` and, on a machine whose `git` has
    // never been resolved, probes for one — neither belongs on the main thread.
    off_async_runtime(move || {
        destination_profile_vms(&destination_profile_table(
            &platform,
            ProfileTableNeed::Chosen,
        ))
    })
    .await
}

/// The picker rows for a profile table, dropping the ones that are not a
/// destination. Split from the command so the filter is asserted without an
/// engine.
fn destination_profile_vms(table: &DestinationProfileTable) -> Vec<RecordingProfileVm> {
    let rows = match table {
        Ok(rows) => rows,
        Err(reason) => {
            tracing::warn!(
                %reason,
                "the synced folders could not be read, so the destination picker offers none"
            );
            return Vec::new();
        }
    };
    rows.iter()
        .filter(|row| row.enabled)
        .filter_map(|row| {
            // One `?` on one field, because a row that cannot say where its
            // recordings live is not a destination — and since DW-196 there is
            // no way for it to name a root without the head beside it.
            let place = row.recordings.as_ref()?;
            Some(RecordingProfileVm {
                id: row.id.clone(),
                name: row.name.clone(),
                recordings_root: place.root.to_string_lossy().into_owned(),
                subfolder: place.subfolder.clone(),
            })
        })
        .collect()
}

/// The rejection a `recording_settings_set` earns for trying to change echo
/// cancellation while a Recording Session is live (Story 22.7).
///
/// Not retriable: the answer does not change until the session ends. Shared
/// with the guard's test so the message the UI shows is asserted, not guessed.
fn echo_cancellation_locked_error() -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: "echo cancellation cannot be changed while a recording is running".to_owned(),
        account_id: None,
        retriable: false,
    }
}

/// Persist the recording settings (Story 17.5 + 19.5 + 40.2 + 41.2, FR-72,
/// FR-131): clamp/normalize to the authored bounds (segment `100..=5000` MB,
/// duration cap `1..=600` min, fps {10, 15, 30, 60} — clamp, not reject), write
/// every value into the `settings` k/v table, and return the effective (re-read)
/// VM so the UI never displays an unsaved value.
///
/// Two fields are REJECTED rather than clamped, because both are specifications
/// rather than data. An unparseable path template earns
/// `IpcErrorCode::RecordingTemplateInvalid` carrying the parse reason. A
/// destination that cannot become a single unambiguous decision earns
/// `IpcErrorCode::RecordingDestinationRefused` naming what it lacks or what it
/// would have collided with ([`destination_choice`]). Either way nothing at all
/// is written. A running session is never mutated — `recording_start` re-reads at
/// start, so edits apply to the next Recording Session only. Failures funnel
/// through [`to_ipc_error`].
///
/// `async` per AD-34-5, with the writes and the re-read in one blocking hop so no
/// other command can observe a half-written settings row from between them.
#[tauri::command]
pub async fn recording_settings_set(
    state: State<'_, AppState>,
    settings: RecordingSettingsVm,
) -> Result<RecordingSettingsVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    // Story 22.7: echo cancellation binds the sidecar's microphone producer
    // once, at Start — a mid-session change could not take effect, and writing
    // it anyway would leave the switch showing a value the running recording
    // does not have. Snapshot liveness here (the `recording_run` guard is not
    // `Send`, so it cannot cross into the blocking hop) and reject BEFORE any
    // write below. Only a CHANGED value is refused: an unrelated edit that
    // round-trips the same echo-cancellation value applies normally.
    let session_live =
        live_snapshot(&state.recording_run).is_some_and(|(snapshot, ..)| snapshot.state.is_live());
    let platform = Arc::clone(&state.platform);
    off_async_runtime(move || {
        write_recording_settings(&data_dir, &settings, session_live, &|need| {
            destination_profile_table(&platform, need)
        })
    })
    .await?
}

/// The blocking body of [`recording_settings_set`]: the Story 22.7 liveness
/// guard, then every write, then the effective re-read — in one hop, so no
/// other command can observe a half-written settings row from between them.
///
/// Split out so the guard is unit-testable without an `AppState`: `session_live`
/// is the caller's snapshot of "a Recording Session is running right now"
/// (`live_snapshot` + `RecordingUiState::is_live`).
fn write_recording_settings(
    data_dir: &Path,
    settings: &RecordingSettingsVm,
    session_live: bool,
    profiles: &dyn Fn(ProfileTableNeed) -> DestinationProfileTable,
) -> Result<RecordingSettingsVm, IpcError> {
    // Reject BEFORE any write: a rejected request must leave the settings table
    // byte-for-byte as it was, not half-applied with only the echo row refused.
    if session_live {
        let stored = keeper_core::registry::get_recording_echo_cancellation(data_dir)
            .map_err(to_ipc_error)?;
        if stored != settings.echo_cancellation {
            return Err(echo_cancellation_locked_error());
        }
    }
    // Story 40.2, same block for the same reason: the eight writes below are
    // sequential, so a template refused halfway through would leave the table
    // holding a new segment size, a new destination and the OLD template. Blank
    // is not a refusal — it clears the key, and the effective read then returns
    // `DEFAULT_TEMPLATE`, which is how "clearing the field restores the
    // documented default" is spelled. Validated, never sanitised: nothing here
    // rewrites the template into one that would have parsed.
    if !settings.path_template.trim().is_empty() {
        PathTemplate::parse(&settings.path_template).map_err(|reason| {
            to_ipc_error(CoreError::Recording(RecordingError::TemplateInvalid {
                reason,
            }))
        })?;
    }
    // Story 41.2, in the same block and for the same reason, one step further:
    // the destination is a DECISION, so what a submission MEANS is settled here,
    // before any row moves. A refusal leaves the table untouched, and an accepted
    // one names exactly one of the two keys — the other is cleared below, which is
    // what makes "exactly one key in force" true by construction rather than by
    // convention.
    let choice = destination_choice(settings, profiles)?;
    keeper_core::registry::set_recording_segment_mb(data_dir, settings.segment_mb)
        .map_err(to_ipc_error)?;
    keeper_core::registry::set_recording_duration_cap_minutes(
        data_dir,
        settings.duration_cap_minutes,
    )
    .map_err(to_ipc_error)?;
    // The losing key is CLEARED, not left behind: a stale folder beside a live
    // profile choice is the ambiguous state the getter has to resolve
    // profile-first, and this command is what keeps it unreachable.
    match &choice {
        DestinationChoice::Folder(dir) => {
            keeper_core::registry::set_recording_destination_dir(data_dir, dir)
                .map_err(to_ipc_error)?;
            keeper_core::registry::set_recording_destination_profile(data_dir, "")
                .map_err(to_ipc_error)?;
        }
        DestinationChoice::Profile(id) => {
            keeper_core::registry::set_recording_destination_profile(data_dir, id)
                .map_err(to_ipc_error)?;
            keeper_core::registry::set_recording_destination_dir(data_dir, "")
                .map_err(to_ipc_error)?;
        }
    }
    keeper_core::registry::set_recording_fps(data_dir, settings.fps).map_err(to_ipc_error)?;
    keeper_core::registry::set_recording_codec(data_dir, &settings.codec).map_err(to_ipc_error)?;
    keeper_core::registry::set_recording_scale_percent(data_dir, settings.scale_percent)
        .map_err(to_ipc_error)?;
    keeper_core::registry::set_recording_echo_cancellation(data_dir, settings.echo_cancellation)
        .map_err(to_ipc_error)?;
    keeper_core::registry::set_recording_path_template(data_dir, &settings.path_template)
        .map_err(to_ipc_error)?;
    read_recording_settings(data_dir, profiles)
}

/// Preview what a path template would name the next recording (Story 40.2,
/// UX-DR45/UX-DR46).
///
/// Read-only in every sense: nothing is parsed into the settings table, nothing
/// is written, and the answer is a projection of (template, title, now,
/// destination root). The settings surface calls this on every keystroke, which
/// is why it is a command at all — the *only* clock that names a session folder
/// is [`chrono::Local::now`] in this crate ([`recording_start`] takes the same
/// one), `keeper-core` is clock-free by contract, and a TypeScript renderer
/// beside 40.1's would be a second implementation of the render rules that
/// could not produce the parse-failure sentences either. One round trip buys
/// the caller a preview that cannot disagree with the recording.
///
/// A template that does not parse is NOT an `Err`: the refusal is the preview's
/// most useful output, and it belongs inline under the field rather than in a
/// rejected promise. `Err` here means the data dir or the destination row could
/// not be read at all.
///
/// `async` per AD-34-5 — a `keeper.db` read per keystroke has no business on the
/// main thread.
#[tauri::command]
pub async fn recording_path_preview(
    state: State<'_, AppState>,
    template: String,
    title: Option<String>,
) -> Result<RecordingPathPreviewVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    // The clock is read HERE, on the way in, exactly as `recording_start` reads
    // it: the preview's promise is "this is where a recording started now would
    // land", and a clock read anywhere below this line would be the same one.
    let ctx = preview_render_ctx(&Local::now(), title.as_deref());
    let platform = Arc::clone(&state.platform);
    off_async_runtime(move || -> Result<RecordingPathPreviewVm, IpcError> {
        let root = effective_destination_dir(&data_dir, &platform);
        Ok(compose_path_preview(&root, &template, &ctx))
    })
    .await?
}

/// The civil datetime + title the preview renders against.
///
/// `seq: 1` — the ordinal is 1-based and 1 adds nothing, so the preview shows
/// the folder the FIRST recording of this minute gets, which is the one the
/// user is about to make. Showing a collision suffix for a collision that has
/// not happened would be a lie about the common case.
///
/// A blank title is `None`, not `Some("")`: the two render identically today,
/// but `None` is what the untitled case actually is, and it is what
/// `recording_start` passes.
///
/// Generic in the timezone, because the six civil numbers are read off whatever
/// zone the `DateTime` already carries. The preview and the start pass a
/// `Local` clock read; Story 40.4's retitle passes the `FixedOffset` its own
/// `startedAt` stamp was written in, so a session re-renders the civil fields
/// the machine saw AT THE START rather than the ones the machine's current zone
/// maps that instant to.
fn preview_render_ctx<Tz: TimeZone>(now: &DateTime<Tz>, title: Option<&str>) -> RenderCtx {
    RenderCtx {
        year: now.year(),
        month: now.month(),
        day: now.day(),
        hour: now.hour(),
        minute: now.minute(),
        second: now.second(),
        title: title
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned),
        seq: 1,
    }
}

/// Compose the preview from a destination root, a raw template and a context.
///
/// Pure — no clock, no registry, no filesystem — so every row of the story's
/// matrix is a unit test with no tempdir behind it.
///
/// Blank in, default out: an emptied field previews [`DEFAULT_TEMPLATE`],
/// because that is exactly what saving an emptied field stores. The rule lives
/// here rather than in the frontend so there is one definition of it, and the
/// preview can never promise something the save would not do.
///
/// The absolute path is built component by component rather than by joining the
/// rendered string: a `RelativePath` is always `/`-separated, and pushing it
/// whole would leave those separators verbatim inside a Windows path.
fn compose_path_preview(root: &Path, template: &str, ctx: &RenderCtx) -> RecordingPathPreviewVm {
    let source = if template.trim().is_empty() {
        DEFAULT_TEMPLATE
    } else {
        template
    };
    match PathTemplate::parse(source) {
        Ok(parsed) => {
            let relative = parsed.render(ctx);
            let mut absolute = root.to_path_buf();
            for component in relative.components() {
                absolute.push(component);
            }
            RecordingPathPreviewVm {
                relative_path: Some(relative.as_str().to_owned()),
                absolute_path: Some(absolute.to_string_lossy().into_owned()),
                problem: None,
            }
        }
        // Both paths stay `None`: a preview that showed a path beside the reason
        // it cannot be used would be inviting the user to believe the path.
        Err(reason) => RecordingPathPreviewVm {
            relative_path: None,
            absolute_path: None,
            problem: Some(reason.to_string()),
        },
    }
}

/// Read whether the one-time iOS no-background-sync disclosure has been shown
/// (Story 14.2, FR-61). Absent ⇒ `false` (not yet shown). The latch is device-global
/// and lives in the `settings` k/v table under `ui.ios_sync_disclosure_shown`.
/// Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn ios_sync_disclosure_shown_get(state: State<'_, AppState>) -> Result<bool, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::get_ios_sync_disclosure_shown(&data_dir).map_err(to_ipc_error)
}

/// Latch the one-time iOS no-background-sync disclosure as shown (Story 14.2, FR-61).
/// Writes `"1"` into the `settings` k/v table — one-way; once acknowledged the card
/// never re-appears, including across relaunch. Failures funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub fn ios_sync_disclosure_shown_set(state: State<'_, AppState>) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::set_ios_sync_disclosure_shown(&data_dir).map_err(to_ipc_error)
}

/// Read whether launch-at-login is enabled (Story 10.3, FR-53, AD-25). The autostart
/// plugin is the single source of truth (its LaunchAgent state), so this reads
/// `autolaunch().is_enabled()` rather than a shadow setting. Default off on a fresh
/// install. Errors funnel through [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn launch_at_login_get(app: tauri::AppHandle) -> Result<bool, IpcError> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| {
        to_ipc_error(CoreError::Internal(format!(
            "could not read autostart: {e}"
        )))
    })
}

/// Mobile stub for [`launch_at_login_get`] (Story 12.2): iOS has no LaunchAgent /
/// autostart concept — an honest `Unsupported` (`retriable: false`). The
/// `launchAtLogin` capability is reported `false`, so Epic 13 hides the toggle.
#[cfg(not(desktop))]
#[tauri::command]
pub fn launch_at_login_get() -> Result<bool, IpcError> {
    Err(to_ipc_error(CoreError::Unsupported(
        "launch-at-login is desktop-only".to_owned(),
    )))
}

/// Set launch-at-login (Story 10.3, FR-53, AD-25). Enables/disables the LaunchAgent
/// through the autostart plugin (authoritative — no shadow source of truth). Off by
/// default; only ever toggled by an explicit user action. Errors funnel through
/// [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn launch_at_login_set(app: tauri::AppHandle, enabled: bool) -> Result<(), IpcError> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| to_ipc_error(CoreError::Internal(format!("could not set autostart: {e}"))))
}

/// Mobile stub for [`launch_at_login_set`] (Story 12.2): iOS has no LaunchAgent /
/// autostart concept — an honest `Unsupported` (`retriable: false`); nothing is
/// toggled or persisted.
#[cfg(not(desktop))]
#[tauri::command]
pub fn launch_at_login_set(enabled: bool) -> Result<(), IpcError> {
    let _ = enabled;
    Err(to_ipc_error(CoreError::Unsupported(
        "launch-at-login is desktop-only".to_owned(),
    )))
}

/// Read the debug-mode toggle (Story 22.5, FR-79) — the LIVE gate, which `init`
/// seeded from the persisted setting and `debug_mode_set` keeps in sync, so the
/// UI always shows what logging is actually doing right now.
#[tauri::command]
pub fn debug_mode_get() -> Result<bool, IpcError> {
    Ok(crate::debug_log::enabled())
}

/// Set the debug-mode toggle (Story 22.5, FR-79): persist `debug.mode` first
/// (durable-before-applied, the settings pattern), then flip the live gate —
/// applies immediately to both the app log and any in-flight session's
/// `events.log`, no restart. Errors funnel through [`to_ipc_error`].
#[tauri::command]
pub fn debug_mode_set(state: State<'_, AppState>, enabled: bool) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    keeper_core::registry::set_debug_mode(&data_dir, enabled).map_err(to_ipc_error)?;
    crate::debug_log::set_enabled(enabled);
    Ok(())
}

/// The tail of the app log, for the in-app diagnostics surface.
///
/// Reads a file the app already writes rather than starting a second stream:
/// warnings and errors are always recorded there (`debug_log`), and everything
/// else joins them while debug mode is on. Returns oldest line first so a
/// viewer can append without reversing.
///
/// No path argument, deliberately: this reads keeper's own log or nothing. A
/// command that took a path would be a file-read primitive handed to the
/// webview, which is a different and much larger thing to be responsible for.
#[tauri::command]
pub fn debug_log_tail(lines: Option<u32>) -> Result<Vec<String>, IpcError> {
    const DEFAULT_LINES: u32 = 200;
    const MAX_LINES: u32 = 2_000;
    let lines = lines.unwrap_or(DEFAULT_LINES).min(MAX_LINES) as usize;
    Ok(crate::debug_log::tail(lines))
}

/// Where that log lives, so the surface can offer "reveal in Finder" and a bug
/// report can name the file.
#[tauri::command]
pub fn debug_log_path() -> Result<String, IpcError> {
    Ok(crate::debug_log::app_log_path().display().to_string())
}

/// Record one stage of an app-driven title-bar drag in the app log (Story 34.3).
///
/// The overlay title bar's drag band cannot report its own failures. Tauri's
/// `data-tauri-drag-region` shim invokes `plugin:window|start_dragging` and
/// drops the returned promise, so a refusal — an ACL denial, or AppKit declining
/// `performWindowDragWithEvent:` because the originating mouse-down is no longer
/// the current event by the time the IPC hop lands — is completely silent: the
/// window simply does not move. The frontend therefore issues that call itself
/// and reports each stage here, where it lands in a file a bug report can carry.
///
/// `WARN`, not `INFO`, deliberately: the file leg of the app log admits
/// `WARN`/`ERROR` regardless of the debug-mode toggle ([`crate::debug_log`]), and
/// that toggle is off by default — an `INFO` line would exist only on a stderr
/// nobody reads once the app is launched from Finder, which is the wrong place
/// for the one thing we need to read back off a user's machine.
///
/// The log text is authored here rather than passed in, so the webview cannot
/// write arbitrary lines into the app log; only `detail` (a refusal message)
/// crosses, capped to one line's worth. Drop this command, and the frontend call
/// that feeds it, once the drag defect is closed.
#[tauri::command]
pub fn titlebar_drag_report(stage: String, detail: Option<String>) {
    // As much of a refusal message as is worth keeping on one log line.
    const MAX_DETAIL_CHARS: usize = 200;
    let detail: String = detail
        .unwrap_or_default()
        .chars()
        .take(MAX_DETAIL_CHARS)
        .collect();
    match stage.as_str() {
        "issued" => tracing::warn!(
            "titlebar drag: start_dragging issued from the drag band (story 34.3 probe)"
        ),
        "accepted" => tracing::warn!(
            "titlebar drag: start_dragging accepted by the window layer (story 34.3 probe)"
        ),
        "refused" => {
            tracing::warn!(%detail, "titlebar drag: start_dragging REFUSED (story 34.3 probe)")
        }
        other => tracing::warn!(
            stage = %other,
            %detail,
            "titlebar drag: unrecognised stage reported by the webview"
        ),
    }
}

/// Read the menu-bar (tray) presence toggle (Story 10.3, FR-53). Reads the persisted
/// `system.menu_bar_presence` setting (default off). Errors funnel through
/// [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn menu_bar_presence_get(state: State<'_, AppState>) -> Result<bool, IpcError> {
    state
        .accounts
        .menu_bar_presence_get(&state.platform)
        .map_err(to_ipc_error)
}

/// Mobile stub for [`menu_bar_presence_get`] (Story 12.2): there is no menu-bar /
/// tray icon on iOS, so presence is honestly `false` regardless of any persisted
/// desktop-written value — the `trayIcon` capability is the single source of truth
/// for surface presence (Epic 13), never this setting.
#[cfg(not(desktop))]
#[tauri::command]
pub fn menu_bar_presence_get() -> Result<bool, IpcError> {
    Ok(false)
}

/// Set the menu-bar (tray) presence toggle (Story 10.3, FR-53). Persists into the
/// `settings` k/v table under `system.menu_bar_presence`, then creates or destroys the
/// tray icon live through the app handle. Off by default; only ever toggled by an
/// explicit user action. Errors funnel through [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn menu_bar_presence_set(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), IpcError> {
    // Persist first (durable-before-applied), then reflect the tray live.
    state
        .accounts
        .menu_bar_presence_set(&state.platform, enabled)
        .map_err(to_ipc_error)?;
    crate::tray::set_tray_presence(&app, enabled);
    Ok(())
}

/// Mobile stub for [`menu_bar_presence_set`] (Story 12.2): there is no menu-bar /
/// tray icon on iOS — an honest `Unsupported` (`retriable: false`); nothing is
/// persisted (the desktop-only flag must not silently change from a phone). The
/// `trayIcon` capability is reported `false`, so Epic 13 hides the toggle.
#[cfg(not(desktop))]
#[tauri::command]
pub fn menu_bar_presence_set(enabled: bool) -> Result<(), IpcError> {
    let _ = enabled;
    Err(to_ipc_error(CoreError::Unsupported(
        "the menu-bar (tray) presence is desktop-only".to_owned(),
    )))
}

/// Read the default fold state of a session's spaces (Story 49.3, FR-276). Reads the
/// persisted `sessions.spaces_folded` setting (default off — spaces arrive unfolded).
/// The DEFAULT only: a space somebody folded by hand is remembered in the frontend's
/// `keeper_session_spaces_fold` cookie and never travels through here. Errors funnel
/// through [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_spaces_folded_get(state: State<'_, AppState>) -> Result<bool, IpcError> {
    state
        .accounts
        .sessions_spaces_folded_get(&state.platform)
        .map_err(to_ipc_error)
}

/// Mobile stub for [`sessions_spaces_folded_get`]: the Sessions surface is desktop-only
/// (`sessions_ipc`'s twins all refuse there), so no space is ever rendered on iOS and
/// `false` — nothing arrives folded — is the honest answer, whatever a desktop wrote.
#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_spaces_folded_get() -> Result<bool, IpcError> {
    Ok(false)
}

/// Set the default fold state of a session's spaces (Story 49.3, FR-276). Persists into
/// the `settings` k/v table under `sessions.spaces_folded`. Nothing else moves: spaces
/// the person folded or unfolded by hand keep their recorded answer, and only the ones
/// with nothing recorded follow the new value. Errors funnel through [`to_ipc_error`].
#[cfg(desktop)]
#[tauri::command]
pub fn sessions_spaces_folded_set(
    state: State<'_, AppState>,
    folded: bool,
) -> Result<(), IpcError> {
    state
        .accounts
        .sessions_spaces_folded_set(&state.platform, folded)
        .map_err(to_ipc_error)
}

/// Mobile stub for [`sessions_spaces_folded_set`]: an honest `Unsupported`
/// (`retriable: false`) with nothing persisted — the default for a desktop-only surface
/// must not be changed from a phone that cannot show it.
#[cfg(not(desktop))]
#[tauri::command]
pub fn sessions_spaces_folded_set(folded: bool) -> Result<(), IpcError> {
    let _ = folded;
    Err(to_ipc_error(CoreError::Unsupported(
        "a session's spaces are a desktop-only surface".to_owned(),
    )))
}

/// Read the per-Chat notification mode for `(accountId, roomId)` (Story 10.2). Resolves
/// the account's live `Client` and reads the synced Matrix push-rule mode. A room-not-
/// found / inactive account funnels through [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn chat_notify_mode_get(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<ChatNotifyMode, IpcError> {
    state
        .accounts
        .chat_notify_mode_get(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Set the per-Chat notification mode for `(accountId, roomId)` (Story 10.2). Writes a
/// synced Matrix push rule so the mode survives restart and syncs across devices; the
/// notify handler reads the verdict back per event. `All` clears any per-Chat rule (the
/// "unmute" target). A room-not-found / inactive account, or a push-rule dispatch
/// failure, funnels through [`to_ipc_error`].
#[tauri::command]
pub async fn chat_notify_mode_set(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    mode: ChatNotifyMode,
) -> Result<(), IpcError> {
    state
        .accounts
        .chat_notify_mode_set(&account_id, &room_id, mode)
        .await
        .map_err(to_ipc_error)
}

/// Read the global Incognito default (Story 8.1). Absent = off (Incognito off by
/// default). Errors funnel through [`to_ipc_error`].
#[tauri::command]
pub fn incognito_get_global(state: State<'_, AppState>) -> Result<bool, IpcError> {
    state
        .accounts
        .incognito_get_global(&state.platform)
        .map_err(to_ipc_error)
}

/// Set the global Incognito default (Story 8.1). Persists into the `settings` k/v
/// table; off by default. Errors funnel through [`to_ipc_error`].
#[tauri::command]
pub fn incognito_set_global(state: State<'_, AppState>, enabled: bool) -> Result<(), IpcError> {
    state
        .accounts
        .incognito_set_global(&state.platform, enabled)
        .map_err(to_ipc_error)
}

/// Read the per-Account Incognito override (Story 8.1). Tri-state: `Some(bool)` = an
/// explicit override, `None` = inherit the global scope. Errors funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub fn incognito_get_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<bool>, IpcError> {
    state
        .accounts
        .incognito_get_account(&state.platform, &account_id)
        .map_err(to_ipc_error)
}

/// Set (or clear) the per-Account Incognito override (Story 8.1). `value` is
/// tri-state: `Some(bool)` sets an explicit override, `None` clears it back to inherit
/// the global scope. Errors funnel through [`to_ipc_error`].
#[tauri::command]
pub fn incognito_set_account(
    state: State<'_, AppState>,
    account_id: String,
    value: Option<bool>,
) -> Result<(), IpcError> {
    state
        .accounts
        .incognito_set_account(&state.platform, &account_id, value)
        .map_err(to_ipc_error)
}

/// Set (or clear) the per-Chat Incognito override for `(accountId, roomId)` (Story
/// 8.1). `enabled` is tri-state: `Some(bool)` upserts an explicit override, `None`
/// clears it back to inherit the account/global scope. Errors funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub fn incognito_set_chat(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    enabled: Option<bool>,
) -> Result<(), IpcError> {
    state
        .accounts
        .incognito_set_chat(&state.platform, &account_id, &room_id, enabled)
        .map_err(to_ipc_error)
}

/// Manually mark a room unread (Story 4.1). Delegates to the core, which sets the
/// `m.marked_unread` account-data flag via `Room::set_unread_flag(true)` so the row
/// renders unread and the flag syncs to the user's other Matrix clients. Best-effort:
/// a dispatch failure is logged and swallowed in the core (no UI error), so this
/// resolves `Ok` even then. A room-not-found / inactive account funnels through
/// [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn mark_room_unread(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .mark_room_unread(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Archive a room (Story 4.2). Delegates to the core, which sets the Matrix
/// low-priority tag (`m.lowpriority`) via `Room::set_is_low_priority(true, None)` so
/// the row moves into the Archive window (unless it is unread) and the tag persists
/// and syncs to the user's other Matrix clients. Best-effort: a dispatch failure is
/// logged and swallowed in the core (no UI error), so this resolves `Ok` even then.
/// A room-not-found / inactive account funnels through [`to_ipc_error`] to
/// `TimelineUnavailable`.
#[tauri::command]
pub async fn archive_room(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .archive_room(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Unarchive a room (Story 4.2). Delegates to the core, which clears the Matrix
/// low-priority tag (`m.lowpriority`) via `Room::set_is_low_priority(false, None)` so
/// the row returns to its chronological Inbox position. Best-effort: a dispatch
/// failure is logged and swallowed in the core (no UI error), so this resolves `Ok`
/// even then. A room-not-found / inactive account funnels through [`to_ipc_error`] to
/// `TimelineUnavailable`.
#[tauri::command]
pub async fn unarchive_room(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .unarchive_room(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Favourite a room (Story 4.4, FR-21). Delegates to the core, which sets the
/// Matrix favourite tag (`m.favourite`) via `Room::set_is_favourite(true, None)`.
/// `m.favourite` is a *notable* tag, so the row moves into the Favorites window on
/// the SDK's live re-emit and the tag persists and syncs to the user's other
/// Matrix clients (no out-of-band merger poke). Best-effort: a dispatch failure is
/// logged and swallowed in the core (no UI error), so this resolves `Ok` even
/// then. A room-not-found / inactive account funnels through [`to_ipc_error`] to
/// `TimelineUnavailable`.
#[tauri::command]
pub async fn favourite_room(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .favourite_room(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Unfavourite a room (Story 4.4). Delegates to the core, which clears the Matrix
/// favourite tag (`m.favourite`) via `Room::set_is_favourite(false, None)` so the
/// row returns to its chronological Inbox position on the SDK's live re-emit.
/// Best-effort: a dispatch failure is logged and swallowed in the core (no UI
/// error), so this resolves `Ok` even then. A room-not-found / inactive account
/// funnels through [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn unfavourite_room(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .unfavourite_room(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Registry key for the Favorites section's persisted collapse/expand state
/// (Story 4.4). Stored as `"true"`/`"false"` in the app-level `settings` table;
/// unset means the section defaults to expanded.
const FAVORITES_COLLAPSED_KEY: &str = "favorites_collapsed";

/// Read the Favorites section's persisted collapse state (Story 4.4). Pure UI
/// chrome (not Matrix state), so it lives in the app-level `settings` table in
/// `keeper.db` (survives restart and re-login). Returns `false` (expanded) when
/// the setting is unset or not `"true"`. A registry error funnels through
/// [`to_ipc_error`].
#[tauri::command]
pub async fn get_favorites_collapsed(state: State<'_, AppState>) -> Result<bool, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let value = keeper_core::registry::get_setting(&data_dir, FAVORITES_COLLAPSED_KEY)
        .map_err(to_ipc_error)?;
    Ok(value.as_deref() == Some("true"))
}

/// Persist the Favorites section's collapse state (Story 4.4). Stores
/// `"true"`/`"false"` in the app-level `settings` table so it survives restart and
/// re-login. A registry error funnels through [`to_ipc_error`].
#[tauri::command]
pub async fn set_favorites_collapsed(
    state: State<'_, AppState>,
    collapsed: bool,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let value = if collapsed { "true" } else { "false" };
    keeper_core::registry::set_setting(&data_dir, FAVORITES_COLLAPSED_KEY, value)
        .map_err(to_ipc_error)
}

/// A pinned-room reference in a reorder request (Story 4.3). Deserialized from the
/// frontend's `{ accountId, roomId }` (camelCase over IPC).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinRef {
    account_id: String,
    room_id: String,
}

/// Pin a room (Story 4.3, FR-22). Delegates to the core, which appends the pin at
/// the end of the keeper-local ordered list, persists it to `keeper.db`, and
/// re-emits the Pins/Inbox/Archive windows so the strip updates within one frame.
/// Best-effort: callers may fire-and-forget and swallow rejection. A registry
/// error funnels through [`to_ipc_error`].
#[tauri::command]
pub async fn pin_room(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    state
        .accounts
        .pin_room(&data_dir, &account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Unpin a room (Story 4.3). Delegates to the core, which removes the keeper-local
/// pin ref and re-emits the windows so the row returns to its chronological Inbox
/// (or Archive) position. Best-effort; a registry error funnels through
/// [`to_ipc_error`].
#[tauri::command]
pub async fn unpin_room(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    state
        .accounts
        .unpin_room(&data_dir, &account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// Reorder the pins to the exact `order` given (Story 4.3). Delegates to the core,
/// which rewrites the keeper-local order to contiguous `0..n` and re-emits the Pins
/// window in the new order. Best-effort; a registry error funnels through
/// [`to_ipc_error`].
#[tauri::command]
pub async fn reorder_pins(state: State<'_, AppState>, order: Vec<PinRef>) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let refs: Vec<(String, String)> = order
        .into_iter()
        .map(|r| (r.account_id, r.room_id))
        .collect();
    state
        .accounts
        .reorder_pins(&data_dir, &refs)
        .await
        .map_err(to_ipc_error)
}

/// Set (or clear) the account's typing notice in the open room (Story 3.9, 8.2,
/// typing, AD-14, FR-43). Delegates to the core, which resolves the effective
/// Incognito policy and gates the emission through the receipt/typing signals seam —
/// while Incognito applies, zero `m.typing` events leave the machine. Best-effort: a
/// dispatch failure (or a fail-closed scope-read skip) is logged and swallowed in the
/// core (typing is never a UI error), so this resolves `Ok` even then. A
/// room-not-found / inactive account funnels through [`to_ipc_error`] to
/// `TimelineUnavailable`.
#[tauri::command]
pub async fn set_typing(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    typing: bool,
) -> Result<(), IpcError> {
    state
        .accounts
        .set_typing(&state.platform, &account_id, &room_id, typing)
        .await
        .map_err(to_ipc_error)
}

/// Release a PUBLIC read receipt on the open room — the explicit "Mark read publicly"
/// action (Story 8.2, AD-14, FR-45). Delegates to the core, which dispatches exactly
/// one public `m.read` on the room's latest event through the signals seam regardless
/// of the effective Incognito policy (the user chose to acknowledge). Best-effort: a
/// dispatch failure is logged and swallowed in the core (never a UI error), so this
/// resolves `Ok` even then. A room-not-found / inactive account funnels through
/// [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn release_receipt(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .release_receipt(&account_id, &room_id)
        .await
        .map_err(to_ipc_error)
}

/// The data-driven per-Network coupling caveats (Story 8.2, FR-44). Projects the
/// embedded `coupling-caveats.json` into a [`Vec<CouplingCaveatVm>`] the frontend
/// joins to the open room's Network by `networkId` to surface the caveat inline at the
/// Incognito toggle. Read-only, account-agnostic static data; a parse/validation
/// failure in the embedded data file funnels the `BridgeError` through
/// [`to_ipc_error`] to `internal`.
#[tauri::command]
pub fn coupling_caveats() -> Result<Vec<CouplingCaveatVm>, IpcError> {
    keeper_core::bridges::coupling_caveats_catalog().map_err(|e| to_ipc_error(e.into()))
}

/// Back-paginate the open room's timeline (Story 3.9, pagination). Delegates to the
/// core, which fetches up to `numEvents` older events; they arrive back over the
/// room's existing timeline subscription (no second channel). Resolves with
/// whether the homeserver start of the room was reached (no more older history). A
/// room-not-found / no-open-timeline / SDK pagination failure funnels through
/// [`to_ipc_error`] to the retriable `TimelineUnavailable` so the boundary shows a
/// retriable inline error, not an infinite spinner.
#[tauri::command]
pub async fn paginate_backwards(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    num_events: u16,
) -> Result<bool, IpcError> {
    state
        .accounts
        .paginate_backwards(&account_id, &room_id, num_events)
        .await
        .map_err(to_ipc_error)
}

/// Subscribe to the open room's typing notifications (Story 3.9, typing, AD-8,
/// AD-14). Opens a `Channel`, streams a [`TypingBatch`] (the current set of *other*
/// members typing, each with a resolved display name) — an initial empty snapshot,
/// then a batch on every change — and returns the subscription id. The sink
/// forwards each batch to the channel; a closed channel drops the batch. Only
/// opaque user ids + display names cross IPC (NFR-9). A room-not-found / inactive
/// account funnels through [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn typing_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    channel: Channel<TypingBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: TypingBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_typing(&account_id, &room_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one typing subscription, aborting its backend producer task
/// and dropping the SDK typing event handler (AD-19). Idempotent.
#[tauri::command]
pub async fn typing_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_typing(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Subscribe to the open room's live back-pagination status (Story 3.9,
/// pagination, AD-8). Opens a `Channel`, streams a [`PaginationStatusBatch`] (a
/// scalar snapshot: `Paginating`/`Idle` + `hitStart`) — an initial snapshot, then
/// deduped changes — and returns the subscription id. The status drives the honest
/// history-boundary row; older events themselves arrive over the timeline
/// subscription, never here. A room-not-found / no-open-timeline funnels through
/// [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn pagination_status_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    channel: Channel<PaginationStatusBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: PaginationStatusBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_pagination_status(&account_id, &room_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one pagination-status subscription, aborting its backend
/// producer task (AD-19). Idempotent.
#[tauri::command]
pub async fn pagination_status_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_pagination_status(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Report every persisted account that can be restored on launch (FR-8, AD-20).
/// Identity only — delegates to the core, which lists the registry rows and
/// returns each whose Keychain session is present as a non-secret [`AccountVm`]
/// (with hue). Resolves to an empty array on a cold install; a row whose session
/// is gone is skipped, not fatal. No eager activation: the lazy inbox subscribe
/// restores each session. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn session_restore(state: State<'_, AppState>) -> Result<Vec<AccountVm>, IpcError> {
    auth::find_restorable_accounts(state.platform.as_ref()).map_err(to_ipc_error)
}

/// Every folder-sync profile's remote URL, handed to the pure `compute_egress` as
/// plain data (Story 23.7).
///
/// Data rather than a type, because AD-40 keeps `keeper-core` free of `keeper-sync`:
/// the crate that computes the disclosure cannot see a `SyncProfile`, so the shell
/// — the one place that links both — projects the profile set down to the only
/// field the disclosure needs. Read live on every call, never cached, which is what
/// makes adding or removing a profile change the disclosed set immediately.
///
/// **A missing engine is not an error here.** The engine cannot exist without a
/// usable `git` (AD-41), and without the engine no push, fetch or clone can happen
/// in this process — so "no engine" and "no folder-sync egress" are the same fact,
/// and an empty list is the honest answer rather than a swallowed failure. Failing
/// the whole egress list on a machine that simply has no `git` would hide the
/// account destinations too, which is strictly less honest.
///
/// **A failed profile read *is* an error.** Here the engine exists — sync is live —
/// and we cannot say what it reaches. Returning a short list would present a partial
/// disclosure as a complete one, the single thing this surface must never do, so the
/// error propagates and Settings → About says it could not load the list.
#[cfg(desktop)]
fn sync_remote_urls(state: &AppState) -> Result<Vec<String>, IpcError> {
    let engine = match crate::sync::engine(Arc::clone(&state.platform)) {
        Ok(engine) => engine,
        Err(error) => {
            tracing::debug!(
                %error,
                "egress: no folder-sync engine on this machine, so no sync remote is disclosed"
            );
            return Ok(Vec::new());
        }
    };
    let profiles = match engine.list_profiles() {
        Ok(profiles) => profiles,
        Err(error) => return Err(crate::sync_ipc::sync_ipc_error(&error)),
    };
    Ok(profiles.into_iter().map(|p| p.remote_url).collect())
}

/// Mobile twin of [`sync_remote_urls`]: iOS links no folder-sync engine at all
/// (`crate::sync` is `#[cfg(desktop)]` because `keeper-sync` must never reach that
/// target), so there is no remote to disclose and the empty list is the whole truth
/// — the same "iOS adds no new egress endpoints" claim `docs/egress.md` makes.
#[cfg(not(desktop))]
fn sync_remote_urls(state: &AppState) -> Result<Vec<String>, IpcError> {
    let _ = state;
    Ok(Vec::new())
}

/// Report the live set of network destinations keeper contacts (Story 11.2,
/// NFR-11, UX-DR17; Story 23.7; Story 61.1, FR-371). Reads the accounts registry
/// from the same path
/// [`session_restore`] uses — `registry::list_accounts` — projects each row to its
/// `(homeserver_url, Provider)`, reads every folder-sync profile's remote via
/// [`sync_remote_urls`], reads every configured AI provider's base URL via
/// `bots::store::provider_base_urls`, and feeds all three plus the shared
/// [`EGRESS_UPDATE_ENDPOINT`]
/// into the pure `compute_egress`. The result is rendered as UI under Settings →
/// About so keeper's egress claim is verifiable, never asserted: each homeserver
/// (deduped), `api.beeper.com` exactly when a Beeper account exists, each distinct
/// sync remote *host* (never the full remote URL — see `egress::remote_host`), each
/// distinct provider *host* (reduced by that same function), and
/// the update endpoint. A legacy row with no/unknown `provider` tag maps to
/// [`Provider::Password`] — Beeper detection still catches it by host. Every input
/// is read on every call, so adding or removing an account, a profile or a
/// provider changes
/// the disclosed set. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn egress_list(state: State<'_, AppState>) -> Result<Vec<EgressEndpointVm>, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let rows = keeper_core::registry::list_accounts(&data_dir).map_err(to_ipc_error)?;
    let accounts: Vec<(String, Provider)> = rows
        .into_iter()
        .map(|row| {
            // A row created after Story 2.5 carries a durable provider tag; a legacy
            // NULL / unrecognized tag falls back to Password. Beeper detection
            // inside `compute_egress` still surfaces `api.beeper.com` for a legacy
            // Beeper row, so the fallback never omits a real destination.
            let provider = row
                .provider
                .as_deref()
                .and_then(Provider::from_registry_str)
                .unwrap_or(Provider::Password);
            (row.homeserver_url, provider)
        })
        .collect();
    let sync_remotes = sync_remote_urls(&state)?;
    // Read from the same table the Providers surface writes, so a provider added
    // there is disclosed here on the next open with no cache in between. A row
    // whose `kind` this build cannot read contributes nothing: `provider_base_urls`
    // drops it, because keeper cannot say what it would contact.
    let provider_base_urls =
        keeper_core::bots::store::provider_base_urls(&data_dir).map_err(to_ipc_error)?;
    Ok(compute_egress(
        &accounts,
        &sync_remotes,
        &provider_base_urls,
        EGRESS_UPDATE_ENDPOINT,
    ))
}

/// Subscribe to the merged unified inbox across every restorable account (FR-18,
/// AD-20, Story 4.2 + 4.3 + 4.4). Activates each account, opens its room-list
/// stream, and partitions the recency-ordered merge into four [`InboxBatch`]
/// streams over one subscription: the Inbox window over `channel`, the Archive
/// window over `archive`, the Pins window over `pins`, and the Favorites window
/// over `favourites` (each a `Reset` window that updates as accounts sync or as
/// archive/pin/favourite state changes). Returns the inbox subscription id — one
/// `inbox_unsubscribe` tears down all four. Ordering and the four-way split are
/// computed in `keeper-core::inbox`, never in JS. A stream-start failure funnels
/// through [`to_ipc_error`] to `SyncUnavailable`.
#[tauri::command]
pub async fn inbox_subscribe(
    state: State<'_, AppState>,
    channel: Channel<InboxBatch>,
    archive: Channel<InboxBatch>,
    pins: Channel<InboxBatch>,
    favourites: Channel<InboxBatch>,
    spaces: Channel<SpacesSnapshot>,
    networks: Channel<NetworksSnapshot>,
) -> Result<u64, IpcError> {
    let inbox_sink = Box::new(move |batch: InboxBatch| channel.send(batch).is_ok());
    let archive_sink = Box::new(move |batch: InboxBatch| archive.send(batch).is_ok());
    let pins_sink = Box::new(move |batch: InboxBatch| pins.send(batch).is_ok());
    let favourites_sink = Box::new(move |batch: InboxBatch| favourites.send(batch).is_ok());
    // Fifth channel (Story 4.5): the aggregated Space list as a whole snapshot.
    let spaces_sink = Box::new(move |snapshot: SpacesSnapshot| spaces.send(snapshot).is_ok());
    // Sixth channel (Story 4.6): the distinct-Networks list as a whole snapshot.
    let networks_sink = Box::new(move |snapshot: NetworksSnapshot| networks.send(snapshot).is_ok());
    state
        .accounts
        .subscribe_inbox(
            &state.platform,
            inbox_sink,
            archive_sink,
            pins_sink,
            favourites_sink,
            spaces_sink,
            networks_sink,
        )
        .await
        .map_err(to_ipc_error)
}

/// Set (or clear) the ephemeral Space filter on the live merged inbox (Story 4.5,
/// FR-22). Delegates to the core, which pokes the live merger to re-emit all four
/// inbox windows narrowed to the selected Space's joined children (mirrors
/// `reorder_pins`). `account_id`/`space_id` are both present to set a filter, or
/// both `None` to clear it; the selection is `(account_id, space_id)` (ephemeral,
/// never persisted). Best-effort — a no-active-inbox case is a harmless no-op.
#[tauri::command]
pub async fn set_space_filter(
    state: State<'_, AppState>,
    account_id: Option<String>,
    space_id: Option<String>,
) -> Result<(), IpcError> {
    state
        .accounts
        .set_space_filter(account_id.zip(space_id))
        .await;
    Ok(())
}

/// Set (or clear) the ephemeral Network filter on the live merged inbox (Story 4.6,
/// FR-24). Delegates to the core, which pokes the live merger to re-emit all four
/// inbox windows narrowed to rooms bridged to the selected Network (mirrors
/// `set_space_filter`). `network` is `Some(name)` to set a filter (name-keyed,
/// cross-account), or `None` to clear it; the selection is ephemeral (never
/// persisted). Composes AND with any active Space filter. Best-effort — a
/// no-active-inbox case is a harmless no-op.
#[tauri::command]
pub async fn set_network_filter(
    state: State<'_, AppState>,
    network: Option<String>,
) -> Result<(), IpcError> {
    state.accounts.set_network_filter(network).await;
    Ok(())
}

/// Unsubscribe the merged inbox, aborting every per-account producer feeding it
/// (AD-20). Idempotent — a mismatched/unknown id is a no-op.
#[tauri::command]
pub async fn inbox_unsubscribe(
    state: State<'_, AppState>,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state.accounts.unsubscribe_inbox(subscription_id).await;
    Ok(())
}

/// Sign out an account locally (AD-10, Story 1.8). Delegates to the core, which
/// tears down the account's live supervision tasks then deletes exactly its SDK
/// store dir, Keychain session entry, and registry row — no server-side logout,
/// works offline, and is idempotent whether or not the account was ever
/// activated. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn sign_out(state: State<'_, AppState>, account_id: String) -> Result<(), IpcError> {
    state
        .accounts
        .sign_out(&state.platform, &account_id)
        .await
        .map_err(to_ipc_error)
}

/// Deliberately delete one account's local archive (Story 5.7, FR-6). Delegates
/// to the core, which routes the purge through the single serialized archive
/// writer so only the target account's `events` rows and `events_fts` entries are
/// removed — every other account's history stays intact. This is the destructive
/// counterpart to the default keep-archive [`sign_out`]; the caller signs out
/// first, then invokes this. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn delete_account_archive(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), IpcError> {
    tracing::info!(account_id = %account_id, "ipc: delete_account_archive");
    state
        .accounts
        .delete_account_archive(&account_id)
        .await
        .map_err(to_ipc_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ms_is_positive() {
        assert!(now_ms() > 0);
    }

    #[test]
    fn resolve_destination_dir_honors_absolute_and_falls_back_otherwise() {
        // Story 19.5: an absolute persisted folder is honored verbatim.
        let data_dir = Path::new("/var/keeper-data");
        assert_eq!(
            resolve_destination_dir(Some("/Users/x/Recordings".to_owned()), data_dir),
            PathBuf::from("/Users/x/Recordings")
        );
        // A relative (hand-edited/corrupt) value is rejected → an absolute
        // default, keeping the "always a concrete absolute folder" invariant.
        let relative = resolve_destination_dir(Some("relative/path".to_owned()), data_dir);
        assert!(relative.is_absolute());
        assert!(relative.ends_with("keeper"));
        // Unset resolves to the same absolute default.
        let unset = resolve_destination_dir(None, data_dir);
        assert!(unset.is_absolute());
        assert!(unset.ends_with("keeper"));
    }

    // --- recording hotkey commands (Story 20.4) -----------------------------
    //
    // The OS-registration legs of `recording_hotkey_set`/`_clear` need a live
    // `AppHandle` + global-shortcut plugin (mirroring `hotkey_set`, which has the
    // same seam); the command *decisions* — validation, persistence semantics,
    // conflict detection, reveal-target resolution — are factored pure and
    // covered here over temp dirs.

    #[test]
    fn recording_hotkey_registry_set_get_round_trip_and_clear_to_unset() {
        let dir = scan_temp_dir("rec-hotkey");
        // Absent ⇒ the unset default the commands report as `isDefault`.
        assert_eq!(
            keeper_core::registry::get_recording_hotkey(&dir).expect("get absent"),
            ""
        );
        // The set command's persistence leg: set → get round-trips.
        keeper_core::registry::set_recording_hotkey(&dir, "Control+Alt+R").expect("set");
        assert_eq!(
            keeper_core::registry::get_recording_hotkey(&dir).expect("get set"),
            "Control+Alt+R"
        );
        // The clear command's persistence leg: persist "" → unset again.
        keeper_core::registry::set_recording_hotkey(&dir, "").expect("clear");
        assert_eq!(
            keeper_core::registry::get_recording_hotkey(&dir).expect("get cleared"),
            ""
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_recording_accelerator_is_rejected_by_set_validation() {
        // Clearing is a separate command — an empty `set` must be rejected
        // before any registration is touched.
        assert!(validate_recording_hotkey("").is_err(), "empty is rejected");
        assert!(
            validate_recording_hotkey("Foo+").is_err(),
            "malformed is rejected"
        );
        assert!(
            validate_recording_hotkey("Control+Alt+R").is_ok(),
            "a valid chord passes validation"
        );
    }

    #[test]
    fn recording_hotkey_conflict_surfaces_summon_clash_and_curated_shortcuts() {
        // A non-empty chord equal to the summon binding warns (the cross-check).
        assert_eq!(
            recording_hotkey_conflict("Control+Alt+Space", "Control+Alt+Space"),
            Some("Conflicts with the Summon keeper hotkey.".to_owned())
        );
        // The unset (empty) binding never claims a summon clash — even against
        // a hypothetically-empty summon value.
        assert_eq!(recording_hotkey_conflict("", ""), None);
        // A distinct chord is conflict-free.
        assert_eq!(
            recording_hotkey_conflict("Control+Alt+R", "Control+Alt+Space"),
            None
        );
        // A summon clash is caught case-insensitively (same binding to the OS).
        assert_eq!(
            recording_hotkey_conflict("control+alt+space", "Control+Alt+Space"),
            Some("Conflicts with the Summon keeper hotkey.".to_owned())
        );
        // The curated system-shortcut list still applies (reused `known_conflict`).
        assert!(
            recording_hotkey_conflict("Super+Space", "Control+Alt+Space")
                .is_some_and(|warning| warning.contains("Spotlight")),
            "curated Spotlight warning surfaces"
        );
    }

    #[test]
    fn reveal_resolves_to_the_nearest_existing_ancestor() {
        let base = scan_temp_dir("reveal");
        // An existing folder reveals itself.
        assert_eq!(nearest_existing_ancestor(&base), base);
        // A not-yet-created destination resolves up to its nearest existing
        // ancestor — one and several levels deep.
        assert_eq!(nearest_existing_ancestor(&base.join("keeper")), base);
        assert_eq!(
            nearest_existing_ancestor(&base.join("missing/deeper/keeper")),
            base
        );
        // A regular file is NOT a valid reveal ancestor: a destination nested
        // under a file skips the file and resolves to the first real directory.
        let file = base.join("not-a-dir");
        std::fs::write(&file, b"x").expect("write temp file");
        assert_eq!(nearest_existing_ancestor(&file.join("keeper")), base);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_capture_target_maps_each_kind() {
        // Story 19.1: an application target maps to an application manifest
        // CaptureTarget, no display id, and the sidecar ApplicationTarget.
        let (target, display_id, application, _audio_only) =
            resolve_capture_target(Some(RecordingTargetVm::Application {
                pid: 501,
                bundle_id: "com.apple.Safari".to_owned(),
            }));
        assert_eq!(
            target,
            CaptureTarget::application("com.apple.Safari".to_owned(), 501)
        );
        assert_eq!(display_id, None);
        assert_eq!(
            application,
            Some(ApplicationTarget {
                pid: 501,
                bundle_id: "com.apple.Safari".to_owned(),
            })
        );

        // A specific display maps to a display target carrying that id.
        let (target, display_id, application, _audio_only) =
            resolve_capture_target(Some(RecordingTargetVm::Display {
                display_id: Some(7),
            }));
        assert_eq!(target, CaptureTarget::display(Some(7)));
        assert_eq!(display_id, Some(7));
        assert_eq!(application, None);

        // No selection preserves the 16.6 main-display default.
        let (target, display_id, application, _audio_only) = resolve_capture_target(None);
        assert_eq!(target, CaptureTarget::display(None));
        assert_eq!(display_id, None);
        assert_eq!(application, None);
    }

    // --- recovered-sessions scan (Story 20.3) -------------------------------

    /// A unique, empty scratch dir under the OS temp root for a scan test.
    fn scan_temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("keeper-scan-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scan temp dir");
        dir
    }

    /// Write a session folder under `base` with a manifest of the given `status`
    /// and one 100-byte `screen-0000.mov` reconciled into the ledger.
    fn write_session(base: &Path, name: &str, status: ManifestStatus) -> PathBuf {
        let folder = base.join(name);
        let mut manifest = SessionManifest::create(
            folder.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
        )
        .expect("create session folder + manifest");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 100]).expect("segment");
        manifest.reconcile_from_dir().expect("reconcile");
        manifest.set_status(status);
        manifest.write().expect("write manifest");
        folder
    }

    #[test]
    fn scan_lists_recovered_excludes_acknowledged_and_non_recovered() {
        let base = scan_temp_dir("list");
        write_session(&base, "keeper-rec recovered-a", ManifestStatus::Recovered);
        write_session(&base, "keeper-rec recovered-b", ManifestStatus::Recovered);
        // A clean finalize is a terminal but NOT a recovery — never surfaced.
        write_session(&base, "keeper-rec finalized", ManifestStatus::Finalized);
        // A still-recording (or non-session) folder is likewise excluded.
        write_session(&base, "keeper-rec live", ManifestStatus::Recording);
        // A folder with no manifest is silently skipped (best-effort).
        std::fs::create_dir_all(base.join("keeper-rec stray")).expect("stray");

        // Nothing acknowledged yet: both recovered sessions surface, sorted.
        let listed = scan_recovered_sessions(&base, &[]);
        let names: Vec<_> = listed
            .iter()
            .map(|s| s.session_folder.rsplit('/').next().unwrap_or("").to_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "keeper-rec recovered-a".to_owned(),
                "keeper-rec recovered-b".to_owned()
            ]
        );
        // Summary figures come from the manifest: 1 screen segment, 100 bytes.
        assert_eq!(listed[0].screen_segment_count, 1);
        assert_eq!(listed[0].total_bytes, 100);

        // Acknowledging one basename excludes exactly it on the next scan.
        let acked = vec!["keeper-rec recovered-a".to_owned()];
        let after = scan_recovered_sessions(&base, &acked);
        assert_eq!(after.len(), 1);
        assert!(after[0].session_folder.ends_with("keeper-rec recovered-b"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_missing_destination_dir_is_empty() {
        let base = scan_temp_dir("missing");
        let missing = base.join("does-not-exist");
        assert!(scan_recovered_sessions(&missing, &[]).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    // --- Story 40.3: the template nests, so the scan walks and keys relative --

    #[test]
    fn scan_lists_a_nested_recovered_session() {
        // The default template puts a session under `{yyyy}/`, so it is no
        // longer an immediate child of the destination root.
        let base = scan_temp_dir("nested");
        let year = base.join("2026");
        std::fs::create_dir_all(&year).expect("year dir");
        let folder = write_session(&year, "keeper-rec x", ManifestStatus::Recovered);

        let listed = scan_recovered_sessions(&base, &[]);
        assert_eq!(listed.len(), 1, "a nested recovered session must surface");
        assert_eq!(
            listed[0].session_folder,
            folder.to_string_lossy().into_owned()
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_acknowledgement_is_keyed_on_the_root_relative_path() {
        // Two sessions share a leaf under different years: a basename key would
        // make them one entry, so dismissing either would swallow both.
        let base = scan_temp_dir("relative-key");
        let first_year = base.join("2026");
        let second_year = base.join("2027");
        std::fs::create_dir_all(&first_year).expect("2026");
        std::fs::create_dir_all(&second_year).expect("2027");
        let first = write_session(&first_year, "keeper-rec x", ManifestStatus::Recovered);
        let second = write_session(&second_year, "keeper-rec x", ManifestStatus::Recovered);
        assert_eq!(scan_recovered_sessions(&base, &[]).len(), 2);

        // Exactly what `recovered_session_acknowledge` would latch for `first`.
        let key = session_relative_key(&base, &first).expect("relative key");
        assert_eq!(key, "2026/keeper-rec x");

        let after = scan_recovered_sessions(&base, &[key]);
        assert_eq!(
            after.len(),
            1,
            "only the acknowledged session is suppressed"
        );
        assert_eq!(
            after[0].session_folder,
            second.to_string_lossy().into_owned()
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_honours_a_pre_nesting_basename_acknowledgement() {
        // Backward compatibility: every entry written before Story 40.3 is a
        // bare basename. A flat session's root-relative path IS its basename,
        // so the old entry still suppresses exactly what it was written for.
        let base = scan_temp_dir("legacy-ack");
        write_session(&base, "keeper-rec flat", ManifestStatus::Recovered);
        write_session(&base, "keeper-rec other", ManifestStatus::Recovered);

        let legacy = vec!["keeper-rec flat".to_owned()];
        let after = scan_recovered_sessions(&base, &legacy);
        assert_eq!(after.len(), 1);
        assert!(after[0].session_folder.ends_with("keeper-rec other"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_ignores_dot_dirs_and_manifest_less_strays() {
        let base = scan_temp_dir("strays");
        // `.Trash` is the OS's, not the destination's: never descended into,
        // so a deleted session cannot come back as a card.
        let trashed = base.join(".Trash");
        std::fs::create_dir_all(&trashed).expect(".Trash");
        write_session(&trashed, "keeper-rec deleted", ManifestStatus::Recovered);
        // A manifest-less tree is walked and ignored — no card, no error.
        std::fs::create_dir_all(base.join("Screenshots").join("2026")).expect("stray tree");
        let year = base.join("2026");
        std::fs::create_dir_all(&year).expect("year dir");
        write_session(&year, "keeper-rec real", ManifestStatus::Recovered);

        let listed = scan_recovered_sessions(&base, &[]);
        assert_eq!(listed.len(), 1, "only the real session surfaces");
        assert!(listed[0].session_folder.ends_with("2026/keeper-rec real"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_stops_at_the_depth_cap() {
        let base = scan_temp_dir("depth");
        // Exactly at the cap (8 components below the root): still a candidate.
        let at_cap = base.join("d1/d2/d3/d4/d5/d6/d7");
        std::fs::create_dir_all(&at_cap).expect("at-cap parents");
        write_session(&at_cap, "keeper-rec deep", ManifestStatus::Recovered);
        // One component deeper: its parent is never read_dir'd.
        let too_deep = base.join("e1/e2/e3/e4/e5/e6/e7/e8");
        std::fs::create_dir_all(&too_deep).expect("too-deep parents");
        write_session(&too_deep, "keeper-rec deeper", ManifestStatus::Recovered);

        let listed = scan_recovered_sessions(&base, &[]);
        assert_eq!(listed.len(), 1, "the cap is 8 components below the root");
        assert!(listed[0].session_folder.ends_with("keeper-rec deep"));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// [`write_session`] plus Story 40.3's immutable identity in the manifest.
    fn write_session_with_id(
        base: &Path,
        name: &str,
        session_id: &str,
        status: ManifestStatus,
    ) -> PathBuf {
        let folder = base.join(name);
        let mut manifest = SessionManifest::create_with_meta(
            folder.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(keeper_core::recording::SessionMeta {
                session_id: Some(session_id.to_owned()),
                ..Default::default()
            }),
            None,
        )
        .expect("create session folder + manifest");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 100]).expect("segment");
        manifest.reconcile_from_dir().expect("reconcile");
        manifest.set_status(status);
        manifest.write().expect("write manifest");
        folder
    }

    /// A destination root plus its own `keeper.db` data dir, for the
    /// acknowledge round-trips (the latch reads the destination back out of the
    /// registry, so the two have to agree).
    fn ack_temp_dirs(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = scan_temp_dir(tag);
        let base = root.join("dest");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&base).expect("destination root");
        keeper_core::registry::set_recording_destination_dir(&data_dir, &base.to_string_lossy())
            .expect("persist the destination");
        (root, base, data_dir)
    }

    #[test]
    fn acknowledge_latches_the_session_id_and_survives_a_moved_folder() {
        let (root, base, data_dir) = ack_temp_dirs("ack-id");
        let year = base.join("2026");
        std::fs::create_dir_all(&year).expect("year dir");
        let session_id = "01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KYDM0000000000000000000A";
        let folder =
            write_session_with_id(&year, "keeper-rec x", session_id, ManifestStatus::Recovered);
        assert_eq!(scan_recovered_sessions(&base, &[]).len(), 1);

        latch_recovered_session_acknowledgement(&data_dir, &resolution_platform(), &folder)
            .expect("latch the dismissal");
        let acknowledged = keeper_core::registry::get_recovered_sessions_acknowledged(&data_dir)
            .expect("read the seen-set back");
        assert_eq!(
            acknowledged,
            vec![session_id.to_owned()],
            "the identity is latched, not the path the session happens to live at"
        );
        assert!(scan_recovered_sessions(&base, &acknowledged).is_empty());

        // Story 40.4 retitles a session by MOVING its folder, and a user can
        // drag one into another year today. A path-keyed dismissal would be
        // orphaned by either and the card would come back unexplained.
        let new_year = base.join("2027");
        std::fs::create_dir_all(&new_year).expect("new year dir");
        std::fs::rename(&folder, new_year.join("keeper-rec renamed")).expect("move the session");

        assert!(
            scan_recovered_sessions(&base, &acknowledged).is_empty(),
            "a dismissal keyed on the session id survives the folder moving"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn acknowledge_falls_back_to_the_relative_path_without_a_session_id() {
        // A session recorded before Story 40.3 has no identity to key on, so
        // the root-relative path stays the key — and still round-trips.
        let (root, base, data_dir) = ack_temp_dirs("ack-legacy");
        let year = base.join("2026");
        std::fs::create_dir_all(&year).expect("year dir");
        let folder = write_session(&year, "keeper-rec legacy", ManifestStatus::Recovered);
        let other = write_session(&base, "keeper-rec other", ManifestStatus::Recovered);
        assert_eq!(scan_recovered_sessions(&base, &[]).len(), 2);

        latch_recovered_session_acknowledgement(&data_dir, &resolution_platform(), &folder)
            .expect("latch the dismissal");
        let acknowledged = keeper_core::registry::get_recovered_sessions_acknowledged(&data_dir)
            .expect("read the seen-set back");
        assert_eq!(acknowledged, vec!["2026/keeper-rec legacy".to_owned()]);

        let after = scan_recovered_sessions(&base, &acknowledged);
        assert_eq!(
            after.len(),
            1,
            "only the acknowledged session is suppressed"
        );
        assert_eq!(
            after[0].session_folder,
            other.to_string_lossy().into_owned()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn acknowledge_survives_an_unreadable_destination_setting() {
        // The frontend drops the card from local state the moment it calls the
        // command, so a rejected dismiss shows as a success and then resurrects
        // the card later with no explanation. A data dir that is a FILE makes
        // every `keeper.db` read fail — and since Story 41.2 the destination read
        // degrades to the default root rather than erroring, so the latch is
        // skipped one step later (the folder is not under that root) and the
        // dismiss still succeeds. Either way it must never fail on the user.
        let root = scan_temp_dir("ack-no-db");
        let folder = write_session(&root, "keeper-rec x", ManifestStatus::Recovered);
        let data_dir = root.join("not-a-data-dir");
        std::fs::write(&data_dir, b"").expect("occupy the data-dir path with a file");

        assert!(
            latch_recovered_session_acknowledgement(&data_dir, &resolution_platform(), &folder)
                .is_ok(),
            "an unreadable destination setting is a logged no-op, never a failed dismiss"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_descends_past_an_unloadable_intermediate_manifest() {
        let base = scan_temp_dir("stray-manifest");
        let year = base.join("2026");
        std::fs::create_dir_all(&year).expect("year dir");
        // A truncated or hand-edited `manifest.json` in the year folder only
        // NOMINATES a session; the load decides. Treating the probe as the
        // answer made the file a lid: every real session beneath it stayed
        // invisible, permanently.
        std::fs::write(year.join("manifest.json"), b"{ not a manifest").expect("stray manifest");
        let real = write_session(&year, "keeper-rec real", ManifestStatus::Recovered);

        let listed = scan_recovered_sessions(&base, &[]);
        assert_eq!(
            listed.len(),
            1,
            "the session under the stray manifest still surfaces"
        );
        assert_eq!(
            listed[0].session_folder,
            real.to_string_lossy().into_owned()
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_stops_at_its_visit_budget() {
        // Depth cannot bound a root that is WIDE — a Movies library, a sync
        // mount — so the walk spends a directory budget too, and truncates
        // rather than failing when it runs out.
        let base = scan_temp_dir("visits");
        for name in ["a", "b", "c", "d"] {
            write_session(
                &base,
                &format!("keeper-rec {name}"),
                ManifestStatus::Recovered,
            );
        }
        assert_eq!(scan_recovered_sessions(&base, &[]).len(), 4);

        let truncated = scan_recovered_sessions_within(&base, &[], 2);
        assert_eq!(
            truncated.len(),
            2,
            "the budget is spent per directory examined, and stops the walk"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn session_summary_reports_manifest_authoritative_figures() {
        let base = scan_temp_dir("summary");
        let folder = write_session(&base, "keeper-rec summary", ManifestStatus::Finalized);
        let summary = recording_session_summary(folder.to_string_lossy().into_owned())
            .await
            .expect("summary");
        assert!(summary.session_folder.ends_with("keeper-rec summary"));
        assert_eq!(summary.screen_segment_count, 1);
        assert_eq!(summary.total_bytes, 100);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A `Stopping` snapshot for the quit-finalize tests (Story 18.2).
    fn stopping_status() -> Arc<Mutex<RecordingStatusVm>> {
        let mut snapshot = RecordingStatusVm::idle();
        snapshot.state = RecordingUiState::Stopping;
        Arc::new(Mutex::new(snapshot))
    }

    // --- Story 18.4: loud-failure triad (fold / fallback / acknowledge) -----

    /// A capturing [`Platform`] double recording every `(title, body)` posted
    /// through `notify`, so the triad's notification leg is assertable without
    /// an OS notifier (mirrors `keeper-core::notify`'s test double).
    struct CapturingPlatform {
        calls: Mutex<Vec<(String, String)>>,
    }

    impl CapturingPlatform {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().expect("lock calls").clone()
        }
    }

    impl Platform for CapturingPlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(PathBuf::from("/tmp/keeper-ipc-test"))
        }
        fn keychain_set(&self, _key: &str, _value: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn keychain_get(&self, _key: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
        fn keychain_delete(&self, _key: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn open_url(&self, _url: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn notify(&self, title: &str, body: &str, _target: &NotifyTarget) -> Result<(), CoreError> {
            self.calls
                .lock()
                .expect("lock calls")
                .push((title.to_owned(), body.to_owned()));
            Ok(())
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("sidecar unused in tests".to_owned()))
        }
        fn exclude_from_backup(&self, _path: &Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn set_badge_count(&self, _count: Option<u32>) -> Result<(), CoreError> {
            Ok(())
        }
    }

    /// A fresh live-fold harness: machine + shared snapshot, driven to the
    /// `Recording` state through the real `fold_recording_event` path.
    fn live_fold_harness(
        platform: &CapturingPlatform,
    ) -> (RecordingSession, Mutex<RecordingStatusVm>) {
        let mut machine = RecordingSession::new();
        let status = Mutex::new({
            let mut snapshot = RecordingStatusVm::idle();
            snapshot.state = RecordingUiState::Preflight;
            snapshot
        });
        assert!(fold_recording_event(
            &mut machine,
            &status,
            platform,
            RecordingEvent::PreflightStarted,
        ));
        assert!(fold_recording_event(
            &mut machine,
            &status,
            platform,
            RecordingEvent::CaptureStarted,
        ));
        assert!(platform.calls().is_empty(), "no notification while healthy");
        (machine, status)
    }

    /// Story 18.4 induced-fault legs (recorder-kill / writer-stall /
    /// device-loss): each synthetic sidecar `error` event drives the machine to
    /// terminal `Failed`, sets the honest `error` on the snapshot, and fires the
    /// fault notification exactly once — the automatable half of the triad (the
    /// tray/banner legs render this same snapshot; see tray/banner tests).
    #[test]
    fn induced_fault_legs_fail_the_snapshot_and_notify_once() {
        for reason in [
            "keeper-rec exited unexpectedly",       // recorder-kill
            "writer stalled — no samples appended", // writer-stall
            "capture device lost",                  // device-loss
        ] {
            let platform = CapturingPlatform::new();
            let (mut machine, status) = live_fold_harness(&platform);
            assert!(fold_recording_event(
                &mut machine,
                &status,
                &platform,
                RecordingEvent::Failed {
                    message: reason.to_owned(),
                },
            ));
            let snapshot = status_lock(&status).clone();
            assert_eq!(snapshot.state, RecordingUiState::Failed, "{reason}");
            assert_eq!(snapshot.error.as_deref(), Some(reason), "{reason}");
            let calls = platform.calls();
            assert_eq!(calls.len(), 1, "exactly one notification for {reason}");
            assert_eq!(calls[0].0, "Recording failed");
            assert!(calls[0].1.contains(reason), "body names the reason");
        }
    }

    /// A second `Failed` event against the already-terminal machine is rejected
    /// by `apply` — the snapshot keeps the first honest message and NO second
    /// notification fires (the sink half of the notify-once dedup).
    #[test]
    fn second_failed_event_neither_overwrites_nor_renotifies() {
        let platform = CapturingPlatform::new();
        let (mut machine, status) = live_fold_harness(&platform);
        assert!(fold_recording_event(
            &mut machine,
            &status,
            &platform,
            RecordingEvent::Failed {
                message: "keeper-rec exited unexpectedly".to_owned(),
            },
        ));
        assert!(!fold_recording_event(
            &mut machine,
            &status,
            &platform,
            RecordingEvent::Failed {
                message: "a different, later message".to_owned(),
            },
        ));
        let snapshot = status_lock(&status).clone();
        assert_eq!(snapshot.state, RecordingUiState::Failed);
        assert_eq!(
            snapshot.error.as_deref(),
            Some("keeper-rec exited unexpectedly")
        );
        assert_eq!(platform.calls().len(), 1, "no double notification");
    }

    /// Warning onset (Story 19.4 leg closed by 18.4): the FIRST sticky warning
    /// notifies once; a repeat while `warning` is already `Some` updates the
    /// last-write-wins message but never re-fires.
    #[test]
    fn warning_onset_notifies_once_and_sticky_repeat_never_refires() {
        let platform = CapturingPlatform::new();
        let (mut machine, status) = live_fold_harness(&platform);
        assert!(fold_recording_event(
            &mut machine,
            &status,
            &platform,
            RecordingEvent::Warning {
                code: "micLost".to_owned(),
                message: "microphone disconnected — using system default input".to_owned(),
            },
        ));
        {
            let snapshot = status_lock(&status);
            assert_eq!(snapshot.state, RecordingUiState::Recording, "still live");
            assert_eq!(
                snapshot.warning.as_deref(),
                Some("microphone disconnected — using system default input")
            );
        }
        let calls = platform.calls();
        assert_eq!(calls.len(), 1, "one notification on warning onset");
        assert_eq!(calls[0].0, "Recording warning");

        // A later warning updates the sticky message (last-write-wins) but the
        // slot is already `Some` — no second notification.
        assert!(fold_recording_event(
            &mut machine,
            &status,
            &platform,
            RecordingEvent::Warning {
                code: "micLost".to_owned(),
                message: "microphone disconnected — no microphone input".to_owned(),
            },
        ));
        assert_eq!(
            status_lock(&status).warning.as_deref(),
            Some("microphone disconnected — no microphone input")
        );
        assert_eq!(platform.calls().len(), 1, "sticky repeat never re-fires");
    }

    /// A warning then a fault fire one notification EACH (independent onsets) —
    /// and the sticky warning survives on the failed snapshot.
    #[test]
    fn warning_then_fault_notify_independently() {
        let platform = CapturingPlatform::new();
        let (mut machine, status) = live_fold_harness(&platform);
        assert!(fold_recording_event(
            &mut machine,
            &status,
            &platform,
            RecordingEvent::Warning {
                code: "micLost".to_owned(),
                message: "microphone disconnected".to_owned(),
            },
        ));
        assert!(fold_recording_event(
            &mut machine,
            &status,
            &platform,
            RecordingEvent::Failed {
                message: "capture device lost".to_owned(),
            },
        ));
        let snapshot = status_lock(&status).clone();
        assert_eq!(snapshot.state, RecordingUiState::Failed);
        assert_eq!(snapshot.error.as_deref(), Some("capture device lost"));
        assert_eq!(snapshot.warning.as_deref(), Some("microphone disconnected"));
        let calls = platform.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "Recording warning");
        assert_eq!(calls[1].0, "Recording failed");
    }

    /// The run_session-Err fallback (`fail_recording_snapshot`): a live snapshot
    /// flips to `Failed` + `error` with exactly one notification.
    #[test]
    fn run_error_fallback_fails_live_snapshot_and_notifies_once() {
        let platform = CapturingPlatform::new();
        let status = {
            let mut snapshot = RecordingStatusVm::idle();
            snapshot.state = RecordingUiState::Recording;
            Mutex::new(snapshot)
        };
        fail_recording_snapshot(&status, &platform, "keeper-rec could not spawn".to_owned());
        let snapshot = status_lock(&status).clone();
        assert_eq!(snapshot.state, RecordingUiState::Failed);
        assert_eq!(
            snapshot.error.as_deref(),
            Some("keeper-rec could not spawn")
        );
        assert_eq!(platform.calls().len(), 1);
    }

    /// The fallback is guarded on not-already-terminal: after the sink already
    /// settled the session (event-path `Failed`, notification fired), the
    /// fallback leaves the snapshot untouched and never double-notifies — the
    /// fault-via-both-paths dedup of the I/O matrix.
    #[test]
    fn run_error_fallback_never_double_notifies_after_sink_failure() {
        let platform = CapturingPlatform::new();
        let (mut machine, status) = live_fold_harness(&platform);
        assert!(fold_recording_event(
            &mut machine,
            &status,
            &platform,
            RecordingEvent::Failed {
                message: "keeper-rec exited unexpectedly".to_owned(),
            },
        ));
        fail_recording_snapshot(
            &status,
            &platform,
            "keeper-rec exited with a non-zero status".to_owned(),
        );
        let snapshot = status_lock(&status).clone();
        assert_eq!(
            snapshot.error.as_deref(),
            Some("keeper-rec exited unexpectedly"),
            "the event-path message is kept"
        );
        assert_eq!(platform.calls().len(), 1, "one notification total");

        // A clean terminal (`Finalized`) is equally protected from the fallback.
        let finalized = {
            let mut snapshot = RecordingStatusVm::idle();
            snapshot.state = RecordingUiState::Finalized;
            Mutex::new(snapshot)
        };
        fail_recording_snapshot(&finalized, &platform, "late task error".to_owned());
        let snapshot = status_lock(&finalized).clone();
        assert_eq!(snapshot.state, RecordingUiState::Finalized);
        assert_eq!(snapshot.error, None);
        assert_eq!(platform.calls().len(), 1, "still one notification total");
    }

    /// A test-only run slot in the given state (no stop trigger, no driver —
    /// exactly what a settled/synthetic session holds).
    fn run_slot_in(state: RecordingUiState, error: Option<&str>) -> Mutex<Option<RecordingRun>> {
        let mut snapshot = RecordingStatusVm::idle();
        snapshot.state = state;
        snapshot.error = error.map(str::to_owned);
        Mutex::new(Some(RecordingRun {
            stop_tx: None,
            status: Arc::new(Mutex::new(snapshot)),
            driver: None,
            segment_cap_mb: 500,
            destination_dir: PathBuf::from("/tmp/keeper-ipc-test"),
            // A settled/synthetic session with no destination profile behind it:
            // `local`, and no engine is ever asked (Story 41.6).
            durability: None,
        }))
    }

    /// Story 18.4: acknowledging a terminal (failed) session clears the slot —
    /// the next snapshot read is the honest idle default (error/warning gone),
    /// which is what releases the held tray/banner error surfaces.
    #[test]
    fn acknowledge_clears_a_terminal_slot() {
        for state in [
            RecordingUiState::Failed,
            RecordingUiState::Finalized,
            RecordingUiState::Recovered,
        ] {
            let slot = run_slot_in(state, Some("keeper-rec exited unexpectedly"));
            assert!(acknowledge_recording_slot(&slot), "{state:?}");
            assert!(slot_lock(&slot).is_none(), "{state:?} slot cleared");
        }
    }

    /// Story 18.4: acknowledging a LIVE session is a strict no-op — the slot,
    /// its state, and its snapshot are untouched (never a silent stop).
    #[test]
    fn acknowledge_is_a_noop_on_a_live_slot() {
        for state in [
            RecordingUiState::Preflight,
            RecordingUiState::Recording,
            RecordingUiState::Rotating,
            RecordingUiState::Stopping,
        ] {
            let slot = run_slot_in(state, None);
            assert!(!acknowledge_recording_slot(&slot), "{state:?}");
            let guard = slot_lock(&slot);
            let run = guard.as_ref().expect("slot retained");
            assert_eq!(status_lock(&run.status).state, state, "state untouched");
        }
    }

    // --- Story 22.7: echo cancellation ------------------------------------

    /// A throwaway data dir for the registry-backed settings tests.
    fn settings_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "keeper-ipc-echo-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the test data dir");
        dir
    }

    /// The empty profile table (Story 41.2), for every settings test that is not
    /// about the destination CHOICE.
    ///
    /// Injected rather than derived from a platform so nothing here can reach a
    /// real engine — which would open this machine's own `sync.db` as a side
    /// effect of asserting a frame rate — and so the "no synced folders at all"
    /// state, which is most machines, is the default every test runs under.
    fn no_profiles(_need: ProfileTableNeed) -> DestinationProfileTable {
        Ok(Vec::new())
    }

    /// A platform port for the call sites that resolve a destination ROOT.
    ///
    /// Only the engine leg needs one, and no test that uses this stores a
    /// destination profile id, so the lazy profile table is never consulted; the
    /// double keeps that explicit (its data dir is a path nothing is written to).
    fn resolution_platform() -> Arc<dyn Platform> {
        Arc::new(CapturingPlatform::new())
    }

    /// The VM a fresh install reads, with `destination_dir` filled from the
    /// same resolver the read path uses (it is echoed back, never clamped).
    fn settings_vm(dir: &Path, echo_cancellation: bool) -> RecordingSettingsVm {
        let mut vm =
            read_recording_settings(dir, &no_profiles).expect("read the effective settings");
        vm.echo_cancellation = echo_cancellation;
        vm
    }

    /// Story 22.7: a fresh install reads echo cancellation OFF (owner decision
    /// 2026-08-05 — the processing is opt-in), and a write round-trips through
    /// the registry into the effective VM.
    #[test]
    fn recording_settings_read_and_write_carry_echo_cancellation() {
        let dir = settings_temp_dir();
        assert!(
            !read_recording_settings(&dir, &no_profiles)
                .expect("fresh read")
                .echo_cancellation,
            "a fresh install must read echo cancellation off"
        );

        let on = settings_vm(&dir, true);
        let effective = write_recording_settings(&dir, &on, false, &no_profiles)
            .expect("write with no live session");
        assert!(
            effective.echo_cancellation,
            "the effective VM must reflect the write"
        );
        assert!(
            read_recording_settings(&dir, &no_profiles)
                .expect("re-read")
                .echo_cancellation
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Story 22.7: while a session is LIVE, a request that CHANGES echo
    /// cancellation is rejected before any write — not one row moves, not even
    /// the unrelated ones in the same request.
    #[test]
    fn recording_settings_set_rejects_a_changed_echo_cancellation_while_live() {
        let dir = settings_temp_dir();
        let before = read_recording_settings(&dir, &no_profiles).expect("baseline read");
        assert!(!before.echo_cancellation, "baseline is off");

        let mut request = settings_vm(&dir, true);
        // A co-edited field in the SAME request must not sneak through.
        request.fps = 60;
        let error = write_recording_settings(&dir, &request, true, &no_profiles)
            .expect_err("a changed echo cancellation must be rejected while live");
        assert_eq!(error.code, IpcErrorCode::Internal);
        assert!(!error.retriable, "the answer cannot change until stop");
        assert_eq!(
            error.message, "echo cancellation cannot be changed while a recording is running",
            "the honest message the UI shows"
        );

        let after = read_recording_settings(&dir, &no_profiles).expect("post-rejection read");
        assert_eq!(after, before, "a rejected request must write NOTHING");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Story 22.7: while live, a request that leaves echo cancellation ALONE
    /// applies normally — the guard refuses a change, not every edit.
    #[test]
    fn recording_settings_set_applies_other_fields_while_live() {
        let dir = settings_temp_dir();
        let mut request = settings_vm(&dir, false);
        request.fps = 60;
        request.codec = "hevc".to_owned();

        let effective = write_recording_settings(&dir, &request, true, &no_profiles)
            .expect("an unchanged echo value applies");
        assert_eq!(effective.fps, 60);
        assert_eq!(effective.codec, "hevc");
        assert!(!effective.echo_cancellation, "unchanged, and still off");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Story 22.7: only a live session locks the switch. A settled slot
    /// (finalized/failed/recovered) and no slot at all both allow the change —
    /// the liveness snapshot `recording_settings_set` takes is exactly
    /// `live_snapshot` + `is_live`.
    #[test]
    fn only_a_live_session_locks_echo_cancellation() {
        for state in [
            RecordingUiState::Preflight,
            RecordingUiState::Recording,
            RecordingUiState::Rotating,
            RecordingUiState::Stopping,
        ] {
            let slot = run_slot_in(state, None);
            assert!(
                live_snapshot(&slot).is_some_and(|(s, ..)| s.state.is_live()),
                "{state:?} must lock the switch"
            );
        }
        for state in [
            RecordingUiState::Finalized,
            RecordingUiState::Recovered,
            RecordingUiState::Failed,
        ] {
            let slot = run_slot_in(state, None);
            assert!(
                !live_snapshot(&slot).is_some_and(|(s, ..)| s.state.is_live()),
                "{state:?} must NOT lock the switch"
            );
        }
        let empty: Mutex<Option<RecordingRun>> = Mutex::new(None);
        assert!(!live_snapshot(&empty).is_some_and(|(s, ..)| s.state.is_live()));
    }

    /// Story 22.7: `recording_start` populates `MicSelection.echo_cancellation`
    /// from the registry, so the persisted switch is what reaches the wire —
    /// and the key rides ONLY inside the mic block.
    #[test]
    fn start_time_mic_selection_takes_echo_cancellation_from_the_registry() {
        let dir = settings_temp_dir();
        for stored in [true, false] {
            keeper_core::registry::set_recording_echo_cancellation(&dir, stored)
                .expect("persist the switch");
            let echo_cancellation =
                keeper_core::registry::get_recording_echo_cancellation(&dir).expect("read back");
            assert_eq!(echo_cancellation, stored);

            // The exact composition `recording_start` performs.
            let microphone = Some(MicSelection {
                device_id: None,
                echo_cancellation: keeper_core::registry::RECORDING_ECHO_CANCELLATION_DEFAULT,
            })
            .map(|mic| MicSelection {
                echo_cancellation,
                ..mic
            });
            assert_eq!(
                microphone.as_ref().map(|mic| mic.echo_cancellation),
                Some(stored)
            );

            let wire: serde_json::Value =
                serde_json::from_str(&keeper_core::recording::start_recording_request(
                    1,
                    &keeper_core::recording::SessionParams {
                        output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
                        display_id: None,
                        application: None,
                        system_audio: true,
                        microphone,
                        camera: None,
                        segment_mb: 500,
                        max_segment_seconds: 1800,
                        fps: 30,
                        codec: "h264".to_owned(),
                        scale_percent: 100,
                        audio_only: false,
                    },
                ))
                .expect("request is JSON");
            assert_eq!(wire["params"]["echoCancellation"], stored);
        }

        // A mic-off session carries no echo key at all, whatever is stored.
        let wire: serde_json::Value =
            serde_json::from_str(&keeper_core::recording::start_recording_request(
                2,
                &keeper_core::recording::SessionParams {
                    output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
                    display_id: None,
                    application: None,
                    system_audio: true,
                    microphone: None,
                    camera: None,
                    segment_mb: 500,
                    max_segment_seconds: 1800,
                    fps: 30,
                    codec: "h264".to_owned(),
                    scale_percent: 100,
                    audio_only: false,
                },
            ))
            .expect("request is JSON");
        assert!(wire["params"].get("echoCancellation").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Story 40.2: the path template setting and its preview -------------

    /// A fixed civil datetime for the preview tests, so the assertions name the
    /// exact path rather than re-deriving it from the clock they are testing.
    fn preview_ctx(title: Option<&str>) -> RenderCtx {
        RenderCtx {
            year: 2026,
            month: 8,
            day: 5,
            hour: 14,
            minute: 32,
            second: 7,
            title: title.map(str::to_owned),
            seq: 1,
        }
    }

    /// Story 40.2: a fresh install reads the documented default, never `""` and
    /// never the unset sentinel; a stored template that parses is what the read
    /// path returns.
    #[test]
    fn recording_settings_read_carries_the_effective_path_template() {
        let dir = settings_temp_dir();
        assert_eq!(
            read_recording_settings(&dir, &no_profiles)
                .expect("fresh read")
                .path_template,
            DEFAULT_TEMPLATE,
            "an unset template must reach the UI as the documented default"
        );

        keeper_core::registry::set_recording_path_template(&dir, "{yyyy}/{mm}/{dd} {slug}")
            .expect("persist a template");
        assert_eq!(
            read_recording_settings(&dir, &no_profiles)
                .expect("read a stored template")
                .path_template,
            "{yyyy}/{mm}/{dd} {slug}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Story 40.2: `import_config_file` writes every `config.json` key verbatim
    /// and validates nothing, so the READ path has to survive a template that
    /// cannot parse — degrading to the default rather than failing the settings
    /// surface the user would have to use to repair it.
    #[test]
    fn an_unparseable_stored_template_degrades_to_the_default_on_read() {
        for stored in [
            "../escape",
            "/absolute/{yyyy}",
            "{week}",
            "{yyyy}/{slug}",
            "",
        ] {
            assert_eq!(
                resolve_path_template(Some(stored.to_owned())),
                DEFAULT_TEMPLATE,
                "{stored:?} must not survive as the effective template"
            );
        }
        assert_eq!(resolve_path_template(None), DEFAULT_TEMPLATE);
        // …and a template that DOES parse is honored verbatim.
        assert_eq!(
            resolve_path_template(Some("{yyyy}/rec-{slug} {HH}{MM}".to_owned())),
            "{yyyy}/rec-{slug} {HH}{MM}"
        );
    }

    /// Story 40.2: the whole point of putting the parse in the pre-write guard.
    /// A rejected template must leave the table byte-for-byte as it was — not
    /// the six unrelated rows that travelled in the same request either.
    #[test]
    fn recording_settings_set_rejects_a_bad_template_without_writing_anything() {
        let dir = settings_temp_dir();
        let before = read_recording_settings(&dir, &no_profiles).expect("baseline read");

        let mut request = before.clone();
        request.path_template = "../{yyyy}".to_owned();
        // Co-edited fields in the SAME request must not sneak through.
        request.fps = 60;
        request.segment_mb = 1000;

        let error = write_recording_settings(&dir, &request, false, &no_profiles)
            .expect_err("a template that cannot parse must be refused");
        assert_eq!(error.code, IpcErrorCode::RecordingTemplateInvalid);
        assert!(
            !error.retriable,
            "resubmitting it can only fail the same way"
        );
        assert_eq!(
            error.message, "a template cannot contain a \".\" or \"..\" folder",
            "the message is 40.1's own sentence, printed inline beside the field"
        );

        let after = read_recording_settings(&dir, &no_profiles).expect("post-rejection read");
        assert_eq!(after, before, "a rejected request must write NOTHING");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Story 40.2: a template that parses round-trips, and a BLANK one clears
    /// the key rather than storing an empty template — which is how "clearing
    /// the field restores the documented default" is delivered.
    #[test]
    fn recording_settings_set_round_trips_a_template_and_clears_a_blank_one() {
        let dir = settings_temp_dir();
        let mut request = read_recording_settings(&dir, &no_profiles).expect("baseline read");

        request.path_template = "{yyyy}/{mm}/rec-{slug}".to_owned();
        let effective = write_recording_settings(&dir, &request, false, &no_profiles)
            .expect("a valid template");
        assert_eq!(effective.path_template, "{yyyy}/{mm}/rec-{slug}");

        request.path_template = "   ".to_owned();
        let cleared = write_recording_settings(&dir, &request, false, &no_profiles)
            .expect("a blank template");
        assert_eq!(
            cleared.path_template, DEFAULT_TEMPLATE,
            "an emptied field restores the default, it does not store nothing"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Story 41.2: the destination is one resolved decision --------------

    /// An enabled, recordings-flagged profile row rooted at `local`, with the
    /// default `recordings` subfolder Story 41.1 authored, on a disk that is
    /// always there. Story 41.7's rows add a volume with [`removable_row`].
    fn flagged_row(id: &str, name: &str, local: &str) -> DestinationProfileRow {
        DestinationProfileRow {
            id: id.to_owned(),
            name: name.to_owned(),
            local_path: PathBuf::from(local),
            // The root and the head spelled the same way the production join
            // spells them, because this fixture stands in for a real row.
            recordings: Some(RecordingsPlace {
                root: PathBuf::from(local).join("recordings"),
                subfolder: "recordings".to_owned(),
            }),
            enabled: true,
            volume: None,
        }
    }

    /// Story 41.7: the same row, on a removable volume in the given state.
    fn removable_row(
        id: &str,
        name: &str,
        local: &str,
        volume: Option<&str>,
        status: DestinationVolumeStatus,
    ) -> DestinationProfileRow {
        DestinationProfileRow {
            volume: Some(DestinationVolume {
                name: volume.map(str::to_owned),
                status,
            }),
            ..flagged_row(id, name, local)
        }
    }

    /// The `tgdrive` fixture the matrix names, as a one-row table.
    fn tgdrive_table() -> DestinationProfileTable {
        Ok(vec![flagged_row("tgd", "tgdrive", "/Volumes/tg")])
    }

    /// The effective settings, read against a hand-built profile table so every
    /// degrade row is asserted on a machine with no `git` at all.
    fn read_with(dir: &Path, table: DestinationProfileTable) -> RecordingSettingsVm {
        read_recording_settings(dir, &|_| table.clone())
            .expect("the destination read must never fail, whatever the profile answer is")
    }

    /// The plain-folder answer for `dir` — what every degrade must land on.
    fn plain_answer(dir: &Path) -> String {
        resolve_destination_dir(None, dir)
            .to_string_lossy()
            .into_owned()
    }

    /// A submitted VM choosing the synced folder `id`.
    fn profile_request(dir: &Path, id: Option<&str>) -> RecordingSettingsVm {
        let mut vm = settings_vm(dir, false);
        vm.destination_kind = RecordingDestinationKind::Profile;
        vm.destination_profile_id = id.map(str::to_owned);
        vm
    }

    /// A submitted VM choosing the plain folder `folder`.
    fn folder_request(dir: &Path, folder: &str) -> RecordingSettingsVm {
        let mut vm = settings_vm(dir, false);
        vm.destination_kind = RecordingDestinationKind::Folder;
        vm.destination_profile_id = None;
        vm.destination_dir = folder.to_owned();
        vm
    }

    /// Matrix rows 1 and 2: neither key set is today's default, and a plain folder
    /// carries no profile name — the surface stays exactly today's surface.
    #[test]
    fn destination_reads_the_folder_answer_with_no_profile_chosen() {
        let dir = settings_temp_dir();
        let fresh = read_with(&dir, Ok(Vec::new()));
        assert_eq!(fresh.destination_kind, RecordingDestinationKind::Folder);
        assert_eq!(fresh.destination_dir, plain_answer(&dir));
        assert!(Path::new(&fresh.destination_dir).is_absolute());
        assert_eq!(fresh.destination_profile_id, None);
        assert_eq!(fresh.destination_profile_name, None);

        keeper_core::registry::set_recording_destination_dir(&dir, "/Users/x/Recordings")
            .expect("persist a plain folder");
        let chosen = read_with(&dir, tgdrive_table());
        assert_eq!(chosen.destination_kind, RecordingDestinationKind::Folder);
        assert_eq!(chosen.destination_dir, "/Users/x/Recordings");
        assert_eq!(chosen.destination_profile_name, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix rows 3 and 4: a flagged profile resolves to `<local_path>/recordings`
    /// with its name, and a RENAME shows the new name against the same root —
    /// which is what makes storing the id rather than the path load-bearing.
    #[test]
    fn destination_resolves_a_flagged_profile_and_follows_its_rename() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");

        let chosen = read_with(&dir, tgdrive_table());
        assert_eq!(chosen.destination_kind, RecordingDestinationKind::Profile);
        assert_eq!(chosen.destination_dir, "/Volumes/tg/recordings");
        assert_eq!(chosen.destination_profile_id.as_deref(), Some("tgd"));
        assert_eq!(chosen.destination_profile_name.as_deref(), Some("tgdrive"));

        let renamed = read_with(
            &dir,
            Ok(vec![flagged_row("tgd", "tg archive", "/Volumes/tg")]),
        );
        assert_eq!(
            renamed.destination_profile_name.as_deref(),
            Some("tg archive"),
            "the name is resolved from the id on every read, never cached"
        );
        assert_eq!(
            renamed.destination_dir, "/Volumes/tg/recordings",
            "a rename does not move the recordings"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix rows 5, 6 and 7: un-flagged behind our back, deleted, paused, and no
    /// engine at all each degrade to the plain-folder answer — and the read
    /// SUCCEEDS in every one of them. A machine with no `git` still records.
    #[test]
    fn destination_degrades_to_the_folder_answer_for_every_unusable_profile() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");
        let plain = plain_answer(&dir);

        let mut unflagged = flagged_row("tgd", "tgdrive", "/Volumes/tg");
        unflagged.recordings = None;
        let mut paused = flagged_row("tgd", "tgdrive", "/Volumes/tg");
        paused.enabled = false;

        for (label, table) in [
            ("un-flagged behind our back", Ok(vec![unflagged])),
            ("paused", Ok(vec![paused])),
            ("deleted", Ok(Vec::new())),
            (
                "no engine (no usable git)",
                Err("git is not available on this machine".to_owned()),
            ),
        ] {
            let degraded = read_with(&dir, table);
            assert_eq!(
                degraded.destination_kind,
                RecordingDestinationKind::Folder,
                "{label}: the surface must fall back to the folder card"
            );
            assert_eq!(
                degraded.destination_dir, plain,
                "{label}: the resolved root must be the plain-path answer"
            );
            assert_eq!(
                degraded.destination_profile_id, None,
                "{label}: a name or id beside a folder kind would be a half-truth"
            );
            assert_eq!(degraded.destination_profile_name, None, "{label}");
        }
        // The choice itself is NOT rewritten by a read: a profile that comes back
        // (a re-flagged folder, a remounted stick, an installed git) resolves again.
        assert_eq!(
            keeper_core::registry::get_recording_destination_profile(&dir).expect("read the key"),
            Some("tgd".to_owned()),
            "a degraded read is a degrade, not a silent unchoosing"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix row: both keys set — only reachable through a hand-edited
    /// `config.json`, because the setter clears the loser. The profile wins,
    /// deterministically, and the folder row is left alone until the next write.
    #[test]
    fn destination_resolves_profile_first_when_both_keys_are_set() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_dir(&dir, "/Users/x/Recordings")
            .expect("hand-edited folder row");
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("hand-edited profile row");

        let resolved = read_with(&dir, tgdrive_table());
        assert_eq!(resolved.destination_kind, RecordingDestinationKind::Profile);
        assert_eq!(resolved.destination_dir, "/Volumes/tg/recordings");
        assert_eq!(
            resolved.destination_profile_name.as_deref(),
            Some("tgdrive")
        );

        // And the next write settles it: one key in force, the other cleared.
        let request = folder_request(&dir, "/Users/x/Recordings");
        write_recording_settings(&dir, &request, false, &|_| tgdrive_table())
            .expect("a folder outside every synced tree is accepted");
        assert_eq!(
            keeper_core::registry::get_recording_destination_profile(&dir).expect("profile key"),
            None,
            "the next write clears the loser"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One refusal row: what it is called, the profile id submitted, the profile
    /// table the read sees, the fragments the message must contain, and whether
    /// the refusal is retriable. Named because five-tuples are what
    /// `clippy::type_complexity` is for.
    type RefusalCase = (
        &'static str,
        Option<&'static str>,
        DestinationProfileTable,
        Vec<&'static str>,
        bool,
    );

    /// Matrix rows: a profile id that is unknown, paused, not recordings-flagged,
    /// missing, or unverifiable is REFUSED — each naming what it lacks — and
    /// nothing at all is written.
    #[test]
    fn settings_set_refuses_a_profile_that_cannot_hold_recordings() {
        let dir = settings_temp_dir();
        let before = read_with(&dir, tgdrive_table());

        let mut unflagged = flagged_row("tgd", "tgdrive", "/Volumes/tg");
        unflagged.recordings = None;
        let mut paused = flagged_row("tgd", "tgdrive", "/Volumes/tg");
        paused.enabled = false;

        let cases: Vec<RefusalCase> = vec![
            (
                "not recordings-flagged",
                Some("tgd"),
                Ok(vec![unflagged]),
                vec!["tgdrive", "doesn't hold recordings"],
                false,
            ),
            (
                "paused",
                Some("tgd"),
                Ok(vec![paused]),
                vec!["tgdrive", "paused"],
                false,
            ),
            (
                "unknown id",
                Some("nope"),
                tgdrive_table(),
                vec!["not set up on this machine"],
                false,
            ),
            (
                "no id at all",
                None,
                tgdrive_table(),
                vec!["no synced folder was chosen"],
                false,
            ),
            (
                "unverifiable (no usable git)",
                Some("tgd"),
                Err("git is not available on this machine".to_owned()),
                vec!["can't be read on this machine"],
                true,
            ),
        ];
        for (label, id, table, expected, retriable) in cases {
            let request = profile_request(&dir, id);
            let error = write_recording_settings(&dir, &request, false, &|_| table.clone())
                .expect_err(label);
            assert_eq!(
                error.code,
                IpcErrorCode::RecordingDestinationRefused,
                "{label}: the surface needs a code it can point at a control with"
            );
            assert_eq!(
                error.retriable, retriable,
                "{label}: only an unreadable engine can succeed on a retry"
            );
            for fragment in expected {
                assert!(
                    error.message.contains(fragment),
                    "{label}: the refusal must say {fragment:?}, got {:?}",
                    error.message
                );
            }
            assert_eq!(
                read_with(&dir, tgdrive_table()),
                before,
                "{label}: a refused request must write NOTHING"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix row "ambiguous plain folder": a folder inside `tgdrive`'s tree that
    /// is not its recordings root is refused, and the refusal NAMES tgdrive —
    /// otherwise the user is told no with no way to find out why.
    #[test]
    fn settings_set_refuses_a_folder_inside_a_synced_tree_naming_the_profile() {
        let dir = settings_temp_dir();
        let before = read_with(&dir, tgdrive_table());

        let request = folder_request(&dir, "/Volumes/tg/inbox");
        let error = write_recording_settings(&dir, &request, false, &|_| tgdrive_table())
            .expect_err("a folder inside a synced tree is the ambiguous case");
        assert_eq!(error.code, IpcErrorCode::RecordingDestinationRefused);
        assert!(
            !error.retriable,
            "resubmitting it can only fail the same way"
        );
        assert!(
            error.message.contains("tgdrive"),
            "the refusal must name the synced folder it would have collided with, got {:?}",
            error.message
        );
        assert!(
            error
                .message
                .contains("choose the synced folder \"tgdrive\" itself"),
            "a flagged collision must offer the choice that carries the consequence, got {:?}",
            error.message
        );
        assert_eq!(
            read_with(&dir, tgdrive_table()),
            before,
            "a refused request must write NOTHING"
        );

        // The same collision with a folder that does NOT hold recordings cannot
        // offer that choice, so it names the other way out instead.
        let mut unflagged = flagged_row("work", "work notes", "/Users/x/work");
        unflagged.recordings = None;
        let table = Ok(vec![unflagged]);
        let inside = folder_request(&dir, "/Users/x/work/screencasts");
        let error = write_recording_settings(&dir, &inside, false, &|_| table.clone())
            .expect_err("an un-flagged synced folder still commits by accident");
        assert!(
            error.message.contains("work notes"),
            "it must still name the folder, got {:?}",
            error.message
        );
        assert!(
            error.message.contains("choose a folder outside it"),
            "with nothing to choose instead, the way out is named, got {:?}",
            error.message
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix row "the unambiguous exception": a plain folder that IS a profile's
    /// recordings root is the same place as the profile choice, and only the
    /// profile choice carries the consequence — so the submission is NORMALISED,
    /// with the folder key cleared.
    #[test]
    fn settings_set_normalises_a_recordings_root_to_the_profile_choice() {
        let dir = settings_temp_dir();
        let request = folder_request(&dir, "/Volumes/tg/recordings");

        let effective = write_recording_settings(&dir, &request, false, &|_| tgdrive_table())
            .expect("the same place, said the other way, is not a refusal");
        assert_eq!(
            effective.destination_kind,
            RecordingDestinationKind::Profile
        );
        assert_eq!(effective.destination_profile_id.as_deref(), Some("tgd"));
        assert_eq!(
            effective.destination_profile_name.as_deref(),
            Some("tgdrive")
        );
        assert_eq!(effective.destination_dir, "/Volumes/tg/recordings");
        assert_eq!(
            keeper_core::registry::get_recording_destination_profile(&dir).expect("profile key"),
            Some("tgd".to_owned()),
            "the PROFILE choice is what is persisted"
        );
        assert_eq!(
            keeper_core::registry::get_recording_destination_dir(&dir).expect("folder key"),
            None,
            "and the folder key is cleared, so exactly one is in force"
        );
        // A trailing separator is the same folder: `Path` compares components.
        let with_slash = folder_request(&dir, "/Volumes/tg/recordings/");
        let again = write_recording_settings(&dir, &with_slash, false, &|_| tgdrive_table())
            .expect("a trailing separator names the same place");
        assert_eq!(again.destination_kind, RecordingDestinationKind::Profile);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix rows "plain folder outside every profile" and the invariant that
    /// binds the two keys: whichever is written, the other is cleared, and a blank
    /// folder submission clears the key back to the DEFAULT answer.
    #[test]
    fn settings_set_keeps_exactly_one_destination_key_in_force() {
        let dir = settings_temp_dir();

        let outside = folder_request(&dir, "/Users/x/Recordings");
        let stored = write_recording_settings(&dir, &outside, false, &|_| tgdrive_table())
            .expect("a folder outside every synced tree is accepted");
        assert_eq!(stored.destination_kind, RecordingDestinationKind::Folder);
        assert_eq!(stored.destination_dir, "/Users/x/Recordings");
        assert_eq!(
            keeper_core::registry::get_recording_destination_profile(&dir).expect("profile key"),
            None
        );

        let synced = profile_request(&dir, Some("tgd"));
        let chosen = write_recording_settings(&dir, &synced, false, &|_| tgdrive_table())
            .expect("a flagged, enabled profile is accepted");
        assert_eq!(chosen.destination_kind, RecordingDestinationKind::Profile);
        assert_eq!(
            keeper_core::registry::get_recording_destination_dir(&dir).expect("folder key"),
            None,
            "choosing a synced folder clears the plain folder"
        );
        assert_eq!(
            keeper_core::registry::get_recording_destination_profile(&dir).expect("profile key"),
            Some("tgd".to_owned())
        );

        // Blank clears BOTH keys, which is how a surface says "no opinion, use the
        // default". What that default IS then depends on the machine, and both
        // answers are asserted here: with one flagged synced folder it resolves to
        // that folder (nothing written), and with none it is `~/Movies/keeper`.
        let blank = folder_request(&dir, "   ");
        let cleared = write_recording_settings(&dir, &blank, false, &|_| tgdrive_table())
            .expect("a blank folder is not a refusal");
        assert_eq!(
            keeper_core::registry::get_recording_destination_dir(&dir).expect("folder key"),
            None
        );
        assert_eq!(
            keeper_core::registry::get_recording_destination_profile(&dir).expect("profile key"),
            None,
            "a blank submission leaves NEITHER key in force"
        );
        assert_eq!(
            cleared.destination_kind,
            RecordingDestinationKind::Profile,
            "with one flagged synced folder, no opinion resolves to it"
        );
        assert_eq!(cleared.destination_dir, "/Volumes/tg/recordings");
        assert_eq!(
            read_with(&dir, Ok(Vec::new())).destination_dir,
            plain_answer(&dir),
            "and on a machine with no flagged folder the same cleared state is today's default"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A machine with no usable `git` must still be able to choose a plain folder:
    /// the collision check is skipped out loud rather than becoming a refusal.
    #[test]
    fn settings_set_accepts_a_plain_folder_with_no_engine_to_check_against() {
        let dir = settings_temp_dir();
        let request = folder_request(&dir, "/Users/x/Recordings");
        let stored = write_recording_settings(&dir, &request, false, &|_| {
            Err("git is not available on this machine".to_owned())
        })
        .expect("capture never degrades because sync is unavailable");
        assert_eq!(stored.destination_dir, "/Users/x/Recordings");
        assert_eq!(stored.destination_kind, RecordingDestinationKind::Folder);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix rows "default when exactly one flagged profile exists" and "no
    /// flagged profile": with NEITHER key set, the one folder that says it holds
    /// recordings IS the destination — and nothing is persisted, so the first
    /// explicit choice still writes as usual. Ambiguity is not a default, and a
    /// paused folder is not one either.
    #[test]
    fn destination_defaults_to_the_only_folder_that_holds_recordings() {
        let dir = settings_temp_dir();

        // Exactly one flagged folder, beside decoys that are not destinations.
        let mut unflagged = flagged_row("work", "work notes", "/Users/x/work");
        unflagged.recordings = None;
        let mut paused = flagged_row("old", "old stick", "/Volumes/old");
        paused.enabled = false;
        let one = Ok(vec![
            unflagged,
            flagged_row("tgd", "tgdrive", "/Volumes/tg"),
            paused,
        ]);

        let defaulted = read_with(&dir, one);
        assert_eq!(
            defaulted.destination_kind,
            RecordingDestinationKind::Profile
        );
        assert_eq!(defaulted.destination_dir, "/Volumes/tg/recordings");
        assert_eq!(defaulted.destination_profile_id.as_deref(), Some("tgd"));
        assert_eq!(
            defaulted.destination_profile_name.as_deref(),
            Some("tgdrive")
        );
        // The whole point: a READ persists nothing. Opening a pane must never
        // redirect anyone's recordings to a remote.
        assert_eq!(
            keeper_core::registry::get_recording_destination_profile(&dir).expect("profile key"),
            None,
            "the default is resolved, never written"
        );
        assert_eq!(
            keeper_core::registry::get_recording_destination_dir(&dir).expect("folder key"),
            None
        );

        // Two flagged folders: no default. Choosing between them here would be a
        // coin toss with a push at the end of it.
        let two = Ok(vec![
            flagged_row("tgd", "tgdrive", "/Volumes/tg"),
            flagged_row("arc", "archive", "/Volumes/arc"),
        ]);
        let ambiguous = read_with(&dir, two);
        assert_eq!(ambiguous.destination_kind, RecordingDestinationKind::Folder);
        assert_eq!(ambiguous.destination_dir, plain_answer(&dir));
        assert_eq!(ambiguous.destination_profile_name, None);

        // One flagged folder, paused: not a destination, so not a default either.
        let mut only_paused = flagged_row("tgd", "tgdrive", "/Volumes/tg");
        only_paused.enabled = false;
        let asleep = read_with(&dir, Ok(vec![only_paused]));
        assert_eq!(asleep.destination_kind, RecordingDestinationKind::Folder);
        assert_eq!(asleep.destination_dir, plain_answer(&dir));

        // No engine at all is no default, and still not an error.
        let no_engine = read_with(&dir, Err("no sync engine is open".to_owned()));
        assert_eq!(no_engine.destination_kind, RecordingDestinationKind::Folder);
        assert_eq!(no_engine.destination_dir, plain_answer(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The default asks for the profile table with a different NEED than a chosen
    /// id does, because the two are allowed to cost different things: a chosen
    /// destination may build the engine, the implicit default may not.
    #[test]
    fn the_default_never_asks_for_an_engine_it_may_build() {
        let dir = settings_temp_dir();
        let asked = Mutex::new(Vec::new());
        let table = |need: ProfileTableNeed| -> DestinationProfileTable {
            asked.lock().expect("lock").push(need);
            tgdrive_table()
        };

        // Neither key set ⇒ the DEFAULT need.
        let _ = read_recording_settings(&dir, &table).expect("read");
        assert_eq!(
            *asked.lock().expect("lock"),
            vec![ProfileTableNeed::Default]
        );

        // An explicit choice ⇒ the CHOSEN need, which may pay for a `git` probe.
        asked.lock().expect("lock").clear();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd").expect("choose");
        let _ = read_recording_settings(&dir, &table).expect("read");
        assert_eq!(*asked.lock().expect("lock"), vec![ProfileTableNeed::Chosen]);

        // An explicit folder answers without asking at all.
        asked.lock().expect("lock").clear();
        keeper_core::registry::set_recording_destination_profile(&dir, "").expect("clear");
        keeper_core::registry::set_recording_destination_dir(&dir, "/Users/x/Recordings")
            .expect("choose a folder");
        let _ = read_recording_settings(&dir, &table).expect("read");
        assert!(
            asked.lock().expect("lock").is_empty(),
            "a stored folder is an answer; nothing needs the synced folders to say so"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Story 41.7: a destination that is sometimes not plugged in --------

    /// The resolved DECISION for a hand-built table, which is what the start
    /// pre-flight judges — `read_with`'s sibling for the half of this story that
    /// never reaches the VM.
    fn resolve_with(dir: &Path, table: DestinationProfileTable) -> RecordingDestination {
        effective_recording_destination(dir, &|_| table.clone())
    }

    /// A throwaway mount root carrying a real volume marker labelled `label`,
    /// plus the profile that lives on it — the fixture for every assertion that
    /// has to go through `volume::scan` rather than a hand-built row.
    fn mounted_volume(label: &str) -> (PathBuf, keeper_sync::SyncProfile) {
        let root = std::env::temp_dir().join(format!(
            "keeper-ipc-volume-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("tgdrive")).expect("create the fake mount root");
        let marker = keeper_sync::volume::VolumeMarker::new(label, 0);
        keeper_sync::volume::VolumeMarker::write(&root, &marker).expect("mint the marker");

        let mut profile = keeper_sync::SyncProfile::new(
            "tgd",
            "tgdrive",
            root.join("tgdrive"),
            "https://example/r.git",
        );
        profile.recordings = Some(keeper_sync::profile::RecordingsConfig::default());
        profile.removable = true;
        profile.volume_id = Some(marker.volume_id.clone());
        (root, profile)
    }

    /// Matrix row "removable destination, attached": it records normally, the
    /// pre-flight has nothing to say, and the card is handed the volume's own name
    /// so it can state that this folder is on a drive before Record is pressed.
    #[test]
    fn an_attached_removable_destination_records_and_says_it_is_removable() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");
        let table = Ok(vec![removable_row(
            "tgd",
            "tgdrive",
            "/Volumes/merope/tgdrive",
            Some("merope"),
            DestinationVolumeStatus::Attached,
        )]);

        let vm = read_with(&dir, table.clone());
        assert_eq!(vm.destination_kind, RecordingDestinationKind::Profile);
        assert_eq!(
            vm.destination_dir, "/Volumes/merope/tgdrive/recordings",
            "an attached drive is an ordinary destination"
        );
        assert_eq!(
            vm.destination_volume,
            Some(RecordingVolumeVm {
                name: Some("merope".to_owned()),
                state: RecordingVolumeState::Attached,
            }),
            "the card must be able to say the folder is on removable media WITHOUT a failure first"
        );
        assert!(
            destination_volume_refusal(&resolve_with(&dir, table)).is_none(),
            "an attached volume is not a reason to refuse a start"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix rows "removable destination, detached" and "card open while
    /// detached": the start is REFUSED by the volume's name, and — the hardest
    /// Never in this story — the destination does NOT quietly become the plain
    /// folder. The card keeps naming the folder the owner chose and says the drive
    /// is not attached.
    #[test]
    fn a_detached_volume_refuses_the_start_by_name_instead_of_redirecting() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");
        let table = Ok(vec![removable_row(
            "tgd",
            "tgdrive",
            "/Volumes/merope/tgdrive",
            Some("merope"),
            DestinationVolumeStatus::Absent,
        )]);

        let vm = read_with(&dir, table.clone());
        assert_eq!(
            vm.destination_kind,
            RecordingDestinationKind::Profile,
            "an unplugged drive is not a fourth degrade: the destination is still the synced folder"
        );
        assert_eq!(
            vm.destination_dir, "/Volumes/merope/tgdrive/recordings",
            "the card must not start naming the plain folder behind the owner's back"
        );
        assert_ne!(vm.destination_dir, plain_answer(&dir));
        assert_eq!(
            vm.destination_volume,
            Some(RecordingVolumeVm {
                name: Some("merope".to_owned()),
                state: RecordingVolumeState::Absent,
            }),
            "the card says the drive is not attached without being asked"
        );

        let error = destination_volume_refusal(&resolve_with(&dir, table))
            .expect("a start onto an unplugged drive must be refused");
        assert_eq!(error.code, IpcErrorCode::RecordingDestinationRefused);
        assert!(
            !error.retriable,
            "nothing keeper does can attach a drive, so an automatic retry would only spin"
        );
        assert!(
            error.message.contains("merope") && error.message.contains("not attached"),
            "the refusal must name the VOLUME — an EPERM on a path is not actionable — got {:?}",
            error.message
        );
        assert!(
            error.message.contains("tgdrive"),
            "and the synced folder it belongs to, got {:?}",
            error.message
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix row "volume returns": the refusal is a statement about right now,
    /// not a latch. Nothing is written when the start is refused, so the next one
    /// after a replug simply works — no re-choosing, no clearing, no relaunch.
    #[test]
    fn replugging_the_volume_lets_the_next_start_succeed_with_no_other_action() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");
        let detached = Ok(vec![removable_row(
            "tgd",
            "tgdrive",
            "/Volumes/merope/tgdrive",
            Some("merope"),
            DestinationVolumeStatus::Absent,
        )]);
        let attached = Ok(vec![removable_row(
            "tgd",
            "tgdrive",
            "/Volumes/merope/tgdrive",
            Some("merope"),
            DestinationVolumeStatus::Attached,
        )]);

        assert!(destination_volume_refusal(&resolve_with(&dir, detached)).is_some());
        assert_eq!(
            keeper_core::registry::get_recording_destination_profile(&dir).expect("profile key"),
            Some("tgd".to_owned()),
            "a refused start is a refusal, not a silent unchoosing"
        );

        let after = resolve_with(&dir, attached);
        assert!(
            destination_volume_refusal(&after).is_none(),
            "the drive is back; the only action required was plugging it in"
        );
        assert_eq!(
            after.root,
            PathBuf::from("/Volumes/merope/tgdrive/recordings")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Matrix row "non-removable profile": an ordinary synced folder carries no
    /// volume at all, so there is no removable wording anywhere and nothing this
    /// story added can refuse a start.
    #[test]
    fn a_non_removable_synced_destination_says_nothing_about_drives() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");

        let vm = read_with(&dir, tgdrive_table());
        assert_eq!(vm.destination_kind, RecordingDestinationKind::Profile);
        assert_eq!(
            vm.destination_volume, None,
            "a folder on a disk that is always there has no drive to talk about"
        );
        assert!(destination_volume_refusal(&resolve_with(&dir, tgdrive_table())).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The regression this story is most able to cause: Story 41.2's three honest
    /// degrades must STILL degrade, even when the profile they are about happens
    /// to be on a drive that is also unplugged. Gone, paused and unflagged mean
    /// "this is not a destination any more" — the folder answer, with a `warn` —
    /// and none of them may become a hard refusal.
    #[test]
    fn the_three_existing_degrades_still_degrade_when_the_volume_is_absent_too() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");
        let plain = plain_answer(&dir);

        let absent = || {
            removable_row(
                "tgd",
                "tgdrive",
                "/Volumes/merope/tgdrive",
                Some("merope"),
                DestinationVolumeStatus::Absent,
            )
        };
        let mut unflagged = absent();
        unflagged.recordings = None;
        let mut paused = absent();
        paused.enabled = false;

        for (label, table) in [
            ("un-flagged behind our back", Ok(vec![unflagged])),
            ("paused", Ok(vec![paused])),
            ("deleted", Ok(Vec::new())),
        ] {
            let vm = read_with(&dir, table.clone());
            assert_eq!(
                vm.destination_kind,
                RecordingDestinationKind::Folder,
                "{label}: 41.2's degrade must survive 41.7"
            );
            assert_eq!(vm.destination_dir, plain, "{label}");
            assert_eq!(
                vm.destination_volume, None,
                "{label}: the plain folder is not on anyone's pendrive"
            );
            assert!(
                destination_volume_refusal(&resolve_with(&dir, table)).is_none(),
                "{label}: a degrade is an answer, and this story must not turn it into a failure"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A volume this run has never had in front of it cannot be named, so the
    /// refusal DESCRIBES it rather than guessing — and in particular never spells
    /// a name out of the mountpoint, which is the one part of a volume's identity
    /// that moves (Story 27.3).
    #[test]
    fn an_unnamed_volume_is_described_rather_than_guessed_at() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");
        let table = Ok(vec![removable_row(
            "tgd",
            "tgdrive",
            "/Volumes/merope/tgdrive",
            None,
            DestinationVolumeStatus::Absent,
        )]);

        let error = destination_volume_refusal(&resolve_with(&dir, table.clone()))
            .expect("an unplugged drive is refused whether or not it can be named");
        assert!(
            error
                .message
                .contains("the removable drive holding the synced folder \"tgdrive\""),
            "with no name to use, the sentence describes the drive, got {:?}",
            error.message
        );
        assert!(
            !error.message.contains("merope") && !error.message.contains("/Volumes"),
            "and it never invents one out of the path, got {:?}",
            error.message
        );
        assert_eq!(
            read_with(&dir, table).destination_volume,
            Some(RecordingVolumeVm {
                name: None,
                state: RecordingVolumeState::Absent,
            }),
            "the card is told the name is unknown, not handed a guess"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A DIFFERENT volume mounted where the profile's own one lives is refused
    /// too, and with its own sentence: `Absent` says "plug it in", this says "look
    /// at what is plugged in". Treating it as absent, or as present, are the two
    /// mistakes `VolumeStatus::Foreign` exists to prevent.
    #[test]
    fn a_foreign_volume_is_refused_with_what_was_found_instead() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");
        let table = Ok(vec![removable_row(
            "tgd",
            "tgdrive",
            "/Volumes/merope/tgdrive",
            Some("merope"),
            DestinationVolumeStatus::Unexpected {
                detail: "a different volume (01STRANGER) is mounted there".to_owned(),
            },
        )]);

        let error = destination_volume_refusal(&resolve_with(&dir, table.clone()))
            .expect("a stranger's drive is not this destination");
        assert!(
            error.message.contains("merope") && error.message.contains("01STRANGER"),
            "the refusal names both what was expected and what was found, got {:?}",
            error.message
        );
        assert_eq!(
            read_with(&dir, table).destination_volume,
            Some(RecordingVolumeVm {
                name: Some("merope".to_owned()),
                state: RecordingVolumeState::Unexpected,
            }),
            "the card distinguishes it from a plain absence"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole loop through the REAL mechanism — `volume::scan`, a real marker,
    /// a real unplug — rather than a hand-built row: the name is learned while the
    /// drive is attached, survives the unplug so the refusal can still say it, and
    /// the refusal creates nothing. Then the drive comes back and the next start
    /// is simply allowed.
    #[test]
    fn a_real_unplug_is_named_refused_and_creates_nothing_until_the_drive_returns() {
        let dir = settings_temp_dir();
        keeper_core::registry::set_recording_destination_profile(&dir, "tgd")
            .expect("choose the synced folder");
        let (root, profile) = mounted_volume("merope");
        let recordings = profile
            .recordings_root()
            .expect("a flagged profile has a root");
        let table = |profile: &keeper_sync::SyncProfile| -> DestinationProfileTable {
            Ok(vec![destination_profile_row(profile)])
        };

        // Attached: the marker is readable, so this is where the name is learned.
        let attached = resolve_with(&dir, table(&profile));
        assert_eq!(attached.root, recordings);
        assert_eq!(
            attached.volume,
            Some(DestinationVolume {
                name: Some("merope".to_owned()),
                status: DestinationVolumeStatus::Attached,
            })
        );
        assert!(destination_volume_refusal(&attached).is_none());

        // Unplugged: the marker goes with the media, exactly as a yanked pendrive
        // takes its `.keeper-sync/volume.json` with it.
        std::fs::remove_dir_all(root.join(keeper_sync::volume::MARKER_DIR)).expect("unplug");
        let detached = resolve_with(&dir, table(&profile));
        assert_eq!(
            detached.volume,
            Some(DestinationVolume {
                name: Some("merope".to_owned()),
                status: DestinationVolumeStatus::Absent,
            }),
            "the status is re-scanned but the NAME is remembered, which is what lets the refusal say it"
        );
        let error =
            destination_volume_refusal(&detached).expect("an unplugged drive refuses the start");
        assert!(error.message.contains("merope"), "{:?}", error.message);
        assert!(
            !recordings.exists(),
            "the refusal must run before anything is created — nothing at all was written"
        );

        // Replugged: same marker, same volume id, and the next start is allowed
        // with no other action taken anywhere.
        let marker = keeper_sync::volume::VolumeMarker {
            schema_version: keeper_sync::volume::MARKER_VERSION,
            volume_id: profile.volume_id.clone().expect("bound"),
            label: "merope".to_owned(),
            profile_ids: vec![profile.id.clone()],
            created_ms: 0,
        };
        keeper_sync::volume::VolumeMarker::write(&root, &marker).expect("replug");
        assert!(
            destination_volume_refusal(&resolve_with(&dir, table(&profile))).is_none(),
            "plugging the drive back in is the only action a replug needs"
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The picker's source: flagged AND enabled only, with the root RESOLVED here
    /// so no surface joins a local path and a subfolder. No engine ⇒ an empty
    /// list, never an error, so the card falls back to today's behaviour.
    ///
    /// Story 46.10: the row also carries the HEAD the root was joined from, and a
    /// multi-segment one is carried verbatim — the Destination card shows and
    /// edits it, so a truncated or re-normalised head would be a value the card
    /// could not echo back to `sync_profile_save`.
    #[test]
    fn destination_profiles_lists_only_the_folders_that_hold_recordings() {
        let mut unflagged = flagged_row("work", "work notes", "/Users/x/work");
        unflagged.recordings = None;
        let mut paused = flagged_row("old", "old stick", "/Volumes/old");
        paused.enabled = false;
        let mut nested = flagged_row("nest", "nested", "/Volumes/nest");
        nested.recordings = Some(RecordingsPlace {
            root: PathBuf::from("/Volumes/nest/40-media/recordings"),
            subfolder: "40-media/recordings".to_owned(),
        });
        let table = Ok(vec![
            flagged_row("tgd", "tgdrive", "/Volumes/tg"),
            unflagged,
            paused,
            nested,
        ]);

        let offered = destination_profile_vms(&table);
        assert_eq!(offered.len(), 2, "only two folders hold recordings");
        assert_eq!(offered[0].id, "tgd");
        assert_eq!(offered[0].name, "tgdrive");
        assert_eq!(offered[0].recordings_root, "/Volumes/tg/recordings");
        assert_eq!(
            offered[0].subfolder, "recordings",
            "the head the root was composed from, beside it"
        );
        assert_eq!(
            offered[1].recordings_root, "/Volumes/nest/40-media/recordings",
            "a nested head resolves to a nested root"
        );
        assert_eq!(
            offered[1].subfolder, "40-media/recordings",
            "a multi-segment head is carried whole, not reduced to its last part"
        );

        assert!(
            destination_profile_vms(&Err("git is not available".to_owned())).is_empty(),
            "no engine offers no profiles, and is never an error"
        );
    }

    /// The collision rule: deepest enabled folder wins (a profile inside another
    /// profile's folder is the repository the file belongs to), and a paused folder
    /// is not a collision at all.
    #[test]
    fn the_enclosing_profile_is_the_deepest_enabled_one() {
        let outer = flagged_row("outer", "outer", "/Volumes/tg");
        let inner = flagged_row("inner", "inner", "/Volumes/tg/nested");
        let mut paused = flagged_row("paused", "paused", "/Volumes/paused");
        paused.enabled = false;
        let rows = vec![outer, inner, paused];

        assert_eq!(
            enclosing_destination_profile(&rows, Path::new("/Volumes/tg/nested/x"))
                .map(|row| row.id.as_str()),
            Some("inner")
        );
        assert_eq!(
            enclosing_destination_profile(&rows, Path::new("/Volumes/tg/x"))
                .map(|row| row.id.as_str()),
            Some("outer")
        );
        assert_eq!(
            enclosing_destination_profile(&rows, Path::new("/Volumes/paused/x")).map(|row| &row.id),
            None,
            "a paused folder is neither a destination nor a collision"
        );
        assert_eq!(
            enclosing_destination_profile(&rows, Path::new("/Users/x/Recordings"))
                .map(|row| &row.id),
            None
        );
    }

    /// The one place a profile becomes a destination row: the recordings root comes
    /// from `keeper-sync` itself, so "where do this profile's recordings live" is
    /// never reimplemented here.
    #[test]
    fn a_profile_row_takes_its_recordings_root_from_the_profile() {
        let mut profile =
            keeper_sync::SyncProfile::new("tgd", "tgdrive", "/Volumes/tg", "https://example/r.git");
        assert_eq!(
            destination_profile_row(&profile).recordings,
            None,
            "a profile that has not said it holds recordings is not a destination"
        );

        profile.recordings = Some(keeper_sync::profile::RecordingsConfig::default());
        let row = destination_profile_row(&profile);
        assert_eq!(row.id, "tgd");
        assert_eq!(row.name, "tgdrive");
        assert_eq!(row.local_path, PathBuf::from("/Volumes/tg"));
        assert_eq!(
            row.recordings_root().map(Path::to_path_buf),
            profile.recordings_root(),
            "one definition of the recordings root, and it is keeper-sync's"
        );
        assert!(row.enabled);
        assert_eq!(
            row.volume, None,
            "a profile that is not on removable media has no volume to talk about"
        );

        // The pendrive the field report is about. The folder does not exist on
        // this machine, so `volume::scan` finds no marker at or above it — which
        // is precisely what a drive in a drawer looks like. The profile has never
        // been bound to a volume, so there is no name to recall either, and the
        // row says so instead of inventing one out of "/Volumes/tg".
        profile.removable = true;
        let removable = destination_profile_row(&profile);
        assert_eq!(
            removable.volume,
            Some(DestinationVolume {
                name: None,
                status: DestinationVolumeStatus::Absent,
            }),
            "a removable profile whose volume is nowhere reads as absent, not as attached"
        );
    }

    /// Story 40.2: the preview is the manual. Titled, untitled and blank-template
    /// all compose the same way, and the absolute line is the destination root
    /// with the rendered components beneath it.
    #[test]
    fn the_path_preview_composes_the_relative_and_absolute_lines() {
        let root = Path::new("/Users/alice/Movies/keeper");

        let titled = compose_path_preview(root, DEFAULT_TEMPLATE, &preview_ctx(Some("Standup")));
        assert_eq!(
            titled.relative_path.as_deref(),
            Some("2026/2026-08-05 1432 standup")
        );
        assert_eq!(
            titled.absolute_path.as_deref(),
            Some("/Users/alice/Movies/keeper/2026/2026-08-05 1432 standup"),
            "the one line of truth: the folder the next recording would use"
        );
        assert_eq!(titled.problem, None);

        // Untitled collapses `{slug}` together with its separator — no trailing
        // space, no "Untitled" placeholder.
        let untitled = compose_path_preview(root, DEFAULT_TEMPLATE, &preview_ctx(None));
        assert_eq!(
            untitled.relative_path.as_deref(),
            Some("2026/2026-08-05 1432")
        );

        // Blank in, default out — the same rule the save path applies, so the
        // preview of an emptied field is the preview of what saving it stores.
        for blank in ["", "   "] {
            assert_eq!(
                compose_path_preview(root, blank, &preview_ctx(Some("Standup"))),
                titled,
                "{blank:?} must preview the default template"
            );
        }

        // A different root moves the absolute line and nothing else.
        let elsewhere = compose_path_preview(
            Path::new("/Volumes/Pendrive"),
            "{yyyy}-{mm}-{dd}",
            &preview_ctx(None),
        );
        assert_eq!(
            elsewhere.absolute_path.as_deref(),
            Some("/Volumes/Pendrive/2026-08-05")
        );
    }

    /// Story 40.2: a template that does not parse previews its REASON and no
    /// path at all — a path beside the reason it cannot be used would invite
    /// the user to believe the path.
    #[test]
    fn the_path_preview_reports_the_parse_reason_and_no_path() {
        let root = Path::new("/Users/alice/Movies/keeper");
        for (template, reason) in [
            (
                "../{yyyy}",
                "a template cannot contain a \".\" or \"..\" folder",
            ),
            (
                "{HH}:{MM}",
                "the character ':' cannot be used in a folder name",
            ),
            (
                "{week}",
                "{week} is not one of the tokens a template understands",
            ),
        ] {
            let preview = compose_path_preview(root, template, &preview_ctx(Some("Standup")));
            assert_eq!(
                preview.problem.as_deref(),
                Some(reason),
                "{template} must preview its own rejection sentence"
            );
            assert_eq!(preview.relative_path, None, "{template}");
            assert_eq!(preview.absolute_path, None, "{template}");
        }
    }

    /// Story 40.2: the preview's clock is the shell's, read once on the way in,
    /// and the ordinal is 1 — the folder the FIRST recording of this minute
    /// gets, not a collision that has not happened.
    #[test]
    fn the_preview_context_mirrors_the_local_clock_at_seq_one() {
        let now = Local::now();
        let ctx = preview_render_ctx(&now, Some("  Standup  "));
        assert_eq!(ctx.year, now.year());
        assert_eq!(ctx.month, now.month());
        assert_eq!(ctx.day, now.day());
        assert_eq!(ctx.hour, now.hour());
        assert_eq!(ctx.minute, now.minute());
        assert_eq!(ctx.second, now.second());
        assert_eq!(ctx.seq, 1, "the ordinal is 1-based and 1 adds nothing");
        assert_eq!(
            ctx.title.as_deref(),
            Some("Standup"),
            "the title arrives trimmed, exactly as `recording_start` trims it"
        );

        // A blank title is the untitled case, not a title that is empty.
        for blank in [None, Some(""), Some("   ")] {
            assert_eq!(preview_render_ctx(&now, blank).title, None, "{blank:?}");
        }
    }

    /// Acknowledge with no session at all is a quiet no-op (idempotent dismiss).
    #[test]
    fn acknowledge_with_empty_slot_is_a_noop() {
        let slot: Mutex<Option<RecordingRun>> = Mutex::new(None);
        assert!(!acknowledge_recording_slot(&slot));
        assert!(slot_lock(&slot).is_none());
    }

    /// Story 34.3 (AD-34-5): the snapshot read is split so its `read_dir`/`stat`
    /// half can leave the calling thread — the main thread on a non-`async`
    /// command, which on macOS is also where `startDragging` resolves. The halves
    /// must still compose into exactly the one authoritative read both the tray and
    /// the command render from, so this pins what each half is responsible for.
    #[test]
    fn the_snapshot_halves_compose_into_one_authoritative_read() {
        // No session this app lifetime: the lock-held half reports nothing, which
        // is what makes both snapshot paths answer the honest idle default.
        let empty: Mutex<Option<RecordingRun>> = Mutex::new(None);
        assert!(live_snapshot(&empty).is_none());

        let slot = run_slot_in(RecordingUiState::Recording, None);
        let (live, segment_cap_mb, durability) =
            live_snapshot(&slot).expect("a live slot yields its snapshot");
        assert!(
            durability.is_none(),
            "a run with no destination profile carries no durability reader"
        );
        assert_eq!(live.state, RecordingUiState::Recording);
        // The cap is SESSION-captured on the run, not on the driver's snapshot, so
        // it has to travel out of the lock alongside it or the meter loses its
        // denominator once the read happens on another thread.
        assert_eq!(segment_cap_mb, 500);
        assert_eq!(
            live.segment_cap_mb, 0,
            "the stored snapshot never carries it"
        );

        // What came out is a clone: the blocking half owns it outright, so a driver
        // write landing meanwhile cannot mutate the value in flight.
        {
            let guard = slot_lock(&slot);
            let run = guard.as_ref().expect("slot retained");
            status_lock(&run.status).state = RecordingUiState::Stopping;
        }
        assert_eq!(live.state, RecordingUiState::Recording);

        // The disk half stamps the cap and reads the real byte figures.
        let folder = scan_temp_dir("snapshot-halves");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 40]).expect("segment");
        let mut recording = live.clone();
        recording.output_path = Some(folder.to_string_lossy().into_owned());
        let enriched = with_disk_figures(recording, segment_cap_mb, None);
        assert_eq!(enriched.segment_cap_mb, 500);
        assert_eq!(enriched.on_disk_bytes, 40);
        assert_eq!(enriched.current_segment_bytes, 40);

        // A session whose folder is gone still yields a snapshot with the cap and
        // zeroed bytes — best-effort, never an error (Story 18.3).
        let _ = std::fs::remove_dir_all(&folder);
        let mut vanished = live.clone();
        vanished.output_path = Some(folder.to_string_lossy().into_owned());
        let enriched = with_disk_figures(vanished, segment_cap_mb, None);
        assert_eq!(enriched.segment_cap_mb, 500);
        assert_eq!(enriched.on_disk_bytes, 0);
        assert_eq!(enriched.current_segment_bytes, 0);
    }

    // --- Story 18.5: live disk-space guard (executor + pre-start gate) ------

    /// One simulated guard tick at the authored thresholds: plan from the
    /// injected `free_bytes` (never a real disk fill), then execute against the
    /// snapshot exactly as the ~1 Hz guard task does — counting stop requests.
    fn disk_guard_tick(
        free_bytes: u64,
        latch: &mut DiskGuardLatch,
        status: &Mutex<RecordingStatusVm>,
        platform: &CapturingPlatform,
        stop_requests: &std::cell::Cell<u32>,
    ) {
        let action = plan_disk_guard_action(
            free_bytes,
            RECORDING_WARN_FREE_BYTES,
            RECORDING_MIN_FREE_BYTES,
            latch,
        );
        apply_disk_guard_action(status, platform, action, || {
            stop_requests.set(stop_requests.get() + 1);
        });
    }

    /// A live `Recording` snapshot for the guard-executor tests.
    fn recording_status() -> Mutex<RecordingStatusVm> {
        let mut snapshot = RecordingStatusVm::idle();
        snapshot.state = RecordingUiState::Recording;
        Mutex::new(snapshot)
    }

    /// Story 18.5 warn leg: a warn-band tick sets the sticky `warning` (the
    /// tray ⚠ line and banner amber render it) and posts exactly one
    /// notification; a second tick still in the warn band re-notifies nothing
    /// and never requests a stop — recording continues.
    #[test]
    fn disk_guard_warn_sets_sticky_warning_and_notifies_once() {
        let platform = CapturingPlatform::new();
        let status = recording_status();
        let stops = std::cell::Cell::new(0u32);
        let mut latch = DiskGuardLatch::default();

        disk_guard_tick(
            RECORDING_WARN_FREE_BYTES - 1,
            &mut latch,
            &status,
            &platform,
            &stops,
        );
        {
            let snapshot = status_lock(&status);
            assert_eq!(snapshot.state, RecordingUiState::Recording, "still live");
            let warning = snapshot.warning.as_deref().expect("sticky warning set");
            assert!(warning.starts_with("Low disk space — "), "{warning}");
            assert!(warning.ends_with(" free"), "{warning}");
        }
        let calls = platform.calls();
        assert_eq!(calls.len(), 1, "one notification on warn onset");
        assert_eq!(calls[0].0, "Recording warning");
        assert!(calls[0].1.contains("Low disk space"), "{}", calls[0].1);
        assert_eq!(stops.get(), 0, "a warn never stops the recording");

        // Warn-sticky: the same band next tick plans `None` — no re-notify.
        disk_guard_tick(
            RECORDING_WARN_FREE_BYTES - 2,
            &mut latch,
            &status,
            &platform,
            &stops,
        );
        assert_eq!(platform.calls().len(), 1, "sticky warn never re-fires");
        assert_eq!(stops.get(), 0);
    }

    /// Story 18.5 hard-floor leg: after a warn already fired, the floor
    /// crossing still notifies (two distinct events), sets the stop reason as
    /// the sticky warning, and requests the graceful stop exactly once — later
    /// floor-band ticks re-issue nothing.
    #[test]
    fn disk_guard_stop_after_warn_notifies_and_requests_stop_once() {
        let platform = CapturingPlatform::new();
        let status = recording_status();
        let stops = std::cell::Cell::new(0u32);
        let mut latch = DiskGuardLatch::default();

        disk_guard_tick(
            RECORDING_WARN_FREE_BYTES - 1,
            &mut latch,
            &status,
            &platform,
            &stops,
        );
        disk_guard_tick(
            RECORDING_MIN_FREE_BYTES - 1,
            &mut latch,
            &status,
            &platform,
            &stops,
        );
        assert_eq!(
            status_lock(&status).warning.as_deref(),
            Some("Recording stopped — low disk"),
            "the stop reason rides the sticky warning (last-write-wins)"
        );
        let calls = platform.calls();
        assert_eq!(calls.len(), 2, "warn onset + stop: one notification EACH");
        // The warn leg is the "still running" warning entry...
        assert_eq!(calls[0].0, "Recording warning");
        assert!(calls[0].1.contains("the recording is still running"));
        // ...but the stop leg uses the dedicated stopped entry: correct title,
        // and NEVER the self-contradicting "still running" suffix (F1).
        assert_eq!(calls[1].0, "Recording stopped");
        assert_eq!(calls[1].1, "Recording stopped — low disk");
        assert!(
            !calls[1].1.contains("still running"),
            "a stop notification must never claim the recording is still running"
        );
        assert_eq!(stops.get(), 1, "exactly one graceful stop request");

        // Post-stop: the guard never re-issues the stop nor re-notifies.
        disk_guard_tick(0, &mut latch, &status, &platform, &stops);
        assert_eq!(platform.calls().len(), 2);
        assert_eq!(stops.get(), 1, "the stop is never re-issued");
    }

    /// Story 18.5 sudden drop: plunging from healthy straight below the floor
    /// in one tick emits the Stop only (the warn is skipped) — one
    /// notification, one stop request.
    #[test]
    fn disk_guard_sudden_drop_stops_without_a_warn() {
        let platform = CapturingPlatform::new();
        let status = recording_status();
        let stops = std::cell::Cell::new(0u32);
        let mut latch = DiskGuardLatch::default();

        disk_guard_tick(u64::MAX, &mut latch, &status, &platform, &stops);
        disk_guard_tick(
            RECORDING_MIN_FREE_BYTES - 1,
            &mut latch,
            &status,
            &platform,
            &stops,
        );
        let calls = platform.calls();
        assert_eq!(
            calls.len(),
            1,
            "stop only — the warn is skipped, not queued"
        );
        assert_eq!(calls[0].0, "Recording stopped");
        assert_eq!(calls[0].1, "Recording stopped — low disk");
        assert!(
            !calls[0].1.contains("still running"),
            "a stop notification must never claim the recording is still running"
        );
        assert_eq!(stops.get(), 1);
    }

    /// Story 18.5 fail-open: a failed probe is reported as `u64::MAX` (plenty)
    /// — the tick is a strict no-op: no warning, no notification, no stop.
    #[test]
    fn disk_guard_failed_probe_is_a_noop() {
        let platform = CapturingPlatform::new();
        let status = recording_status();
        let stops = std::cell::Cell::new(0u32);
        let mut latch = DiskGuardLatch::default();

        disk_guard_tick(u64::MAX, &mut latch, &status, &platform, &stops);
        assert_eq!(
            status_lock(&status).warning,
            None,
            "no warning on fail-open"
        );
        assert!(platform.calls().is_empty(), "no notification on fail-open");
        assert_eq!(stops.get(), 0, "no stop on fail-open");
    }

    /// Story 18.5 pre-start gate: the exact decision + error mapping
    /// `recording_start` runs rejects a Start when the probed free space is
    /// below the hard floor — with the actionable free-space reason, before
    /// any capture begins (retriable: free space and press Start again).
    #[test]
    fn pre_start_gate_rejects_simulated_low_free_space() {
        let reason = evaluate_destination(
            true,
            true,
            RECORDING_MIN_FREE_BYTES - 1,
            RECORDING_MIN_FREE_BYTES,
        )
        .expect_err("below the floor must reject the Start");
        let error = to_ipc_error(CoreError::Recording(RecordingError::DestinationInvalid {
            reason,
        }));
        assert!(
            error.message.contains("not enough free space"),
            "names the free-space reason: {}",
            error.message
        );
        assert!(error.retriable, "the user can free space and retry");
    }

    /// Story 18.2 (paused clock): a sidecar that finalizes inside the bound
    /// resolves `Finalized` — no force-kill leg is reached.
    #[tokio::test(start_paused = true)]
    async fn finalize_within_resolves_when_status_turns_terminal_before_timeout() {
        let status = stopping_status();
        let writer = status.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            status_lock(&writer).state = RecordingUiState::Finalized;
        });
        let outcome =
            finalize_within(&status, Duration::from_secs(10), Duration::from_millis(100)).await;
        assert_eq!(outcome, FinalizeOutcome::Finalized);
    }

    /// Story 18.2 (paused clock): a snapshot that is already terminal at entry
    /// resolves `Finalized` immediately — quit adds no wait at all.
    #[tokio::test(start_paused = true)]
    async fn finalize_within_resolves_immediately_when_already_terminal() {
        let status = Arc::new(Mutex::new(RecordingStatusVm::idle()));
        let before = tokio::time::Instant::now();
        let outcome =
            finalize_within(&status, Duration::from_secs(10), Duration::from_millis(100)).await;
        assert_eq!(outcome, FinalizeOutcome::Finalized);
        assert_eq!(tokio::time::Instant::now(), before, "no sleep on entry");
    }

    /// Story 18.2 (paused clock): a sidecar hung past the bound resolves
    /// `TimedOut` — the caller then aborts the driver (force-kill leg).
    #[tokio::test(start_paused = true)]
    async fn finalize_within_times_out_on_a_hung_sidecar() {
        let status = stopping_status();
        let outcome =
            finalize_within(&status, Duration::from_secs(10), Duration::from_millis(100)).await;
        assert_eq!(outcome, FinalizeOutcome::TimedOut);
    }

    /// Story 14.7 (FR-65): per-path backup exclusion is an iOS-only concept — the
    /// desktop port is an honest no-op that returns `Ok(())` for any path (even one
    /// that does not exist) and never fails a store-creation site.
    #[cfg(desktop)]
    #[test]
    fn desktop_exclude_from_backup_is_a_noop_ok() {
        let platform = DesktopPlatform;
        assert!(platform
            .exclude_from_backup(Path::new("/nonexistent/keeper-test-path"))
            .is_ok());
        assert!(platform.exclude_from_backup(&std::env::temp_dir()).is_ok());
    }

    /// Story 14.4: the `nav_state` slot round-trips through the same helpers the
    /// `nav_state_set`/`nav_state_get`/`nav_state_clear` commands delegate to —
    /// set stores, get reads WITHOUT consuming, clear (take) empties.
    #[test]
    fn nav_state_slot_set_get_clear_round_trip() {
        let slot: Mutex<Option<NavState>> = Mutex::new(None);
        assert_eq!(slot_get(&slot), None, "cold launch: no stored nav");

        let nav = NavState {
            account_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            room_id: "!room:example.org".to_owned(),
            detail_open: true,
        };
        slot_set(&slot, nav.clone());
        assert_eq!(slot_get(&slot), Some(nav.clone()));
        // `get` is a read, not a take — a second read still sees the value
        // (StrictMode-safe: a re-run can never consume it out from under a sibling).
        assert_eq!(slot_get(&slot), Some(nav.clone()));

        // A re-set overwrites (the reporter pushes the latest level).
        let updated = NavState {
            detail_open: false,
            ..nav
        };
        slot_set(&slot, updated.clone());
        assert_eq!(slot_get(&slot), Some(updated.clone()));

        // Clear (take) consumes; clearing again is an idempotent no-op.
        assert_eq!(slot_take(&slot), Some(updated));
        assert_eq!(slot_get(&slot), None);
        assert_eq!(slot_take(&slot), None);
    }

    /// Story 14.4: the slot helpers recover a poisoned lock instead of panicking —
    /// a resume/nav concern must never take the app down.
    #[test]
    fn nav_state_slot_recovers_a_poisoned_lock() {
        let slot: Arc<Mutex<Option<NavState>>> = Arc::new(Mutex::new(Some(NavState {
            account_id: "acct".to_owned(),
            room_id: "!r:example.org".to_owned(),
            detail_open: false,
        })));
        // Poison the lock by panicking while holding it on another thread.
        let poisoner = slot.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock();
            panic!("poison the slot lock");
        })
        .join();
        assert!(slot.is_poisoned(), "the lock should be poisoned");
        assert_eq!(
            slot_get(&slot).map(|nav| nav.room_id),
            Some("!r:example.org".to_owned()),
            "a poisoned slot still reads its stored value"
        );
        assert!(slot_take(&slot).is_some());
        assert_eq!(slot_get(&slot), None);
    }

    /// Egress honesty guard (Story 11.2, NFR-11): the About surface shows
    /// `EGRESS_UPDATE_ENDPOINT`, but the updater actually checks the URL in
    /// `tauri.conf.json` `plugins.updater.endpoints`. If these two literals drift, the
    /// egress list would disclose a destination the app no longer contacts — the exact
    /// dishonesty this story prevents. Fail the build the moment they diverge.
    #[test]
    fn egress_update_endpoint_matches_tauri_conf() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json parses as JSON");
        let endpoints = conf["plugins"]["updater"]["endpoints"]
            .as_array()
            .expect("plugins.updater.endpoints is an array");
        assert!(
            endpoints
                .iter()
                .any(|e| e.as_str() == Some(EGRESS_UPDATE_ENDPOINT)),
            "EGRESS_UPDATE_ENDPOINT ({EGRESS_UPDATE_ENDPOINT}) must appear in \
             tauri.conf.json plugins.updater.endpoints ({endpoints:?}) — keep the egress \
             list and the updater config in sync"
        );
    }

    /// Tauri's `invoke_handler` ASSIGNS the handler it is given
    /// (`self.invoke_handler = Box::new(handler)`) — it does not accumulate. A
    /// second registration therefore silently discards every command the first
    /// one carried, and nothing warns: the build is clean, the app launches, and
    /// each discarded command answers "Command <name> not found" at runtime.
    ///
    /// v0.4.0-v0.4.2 shipped exactly that. Folder sync's desktop-only commands
    /// were registered in a second pass (because `generate_handler!` takes a flat
    /// literal that cannot hold a `#[cfg]` entry), leaving the desktop build with
    /// nine reachable commands and no account restore, no capability probe, no
    /// recording and no bridges. Platform-conditional command sets must be spliced
    /// into the single list — see `keeper_with_commands!` in `lib.rs`.
    #[test]
    fn exactly_one_invoke_handler_is_registered() {
        let src = include_str!("lib.rs");
        let registrations = src.matches(".invoke_handler(").count();
        assert_eq!(
            registrations, 1,
            "lib.rs registers the IPC handler {registrations} times; every call after \
             the first discards the commands registered before it, so all but the last \
             list becomes unreachable at runtime"
        );
    }

    /// The shipped app's version is whatever `tauri.conf.json` says — the bundle's
    /// `CFBundleShortVersionString` and the updater's version comparison both come
    /// from it, NOT from Cargo or npm metadata. So a release that bumps the crate
    /// and `package.json` but forgets this file publishes artifacts named for the
    /// new version containing an app that still reports the old one, and the
    /// updater then offers that "new" version to a machine that just installed it,
    /// forever. Exactly that shipped as v0.4.0. Pin the three together here.
    #[test]
    fn app_version_matches_crate_and_package_manifests() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json parses as JSON");
        let app = conf["version"].as_str().expect("tauri.conf.json version");
        assert_eq!(
            app,
            env!("CARGO_PKG_VERSION"),
            "tauri.conf.json version must match the crate version — the bundle and the \
             updater read the former, release asset names the latter"
        );

        let pkg: serde_json::Value = serde_json::from_str(include_str!("../../../../package.json"))
            .expect("package.json parses as JSON");
        let pkg = pkg["version"].as_str().expect("package.json version");
        assert_eq!(
            app, pkg,
            "package.json version must match the shipped app version"
        );
    }

    #[test]
    fn unsupported_core_error_maps_to_unsupported_code() {
        let ipc = to_ipc_error(CoreError::Unsupported("nope".to_owned()));
        assert_eq!(ipc.code, IpcErrorCode::Unsupported);
        assert!(!ipc.retriable);
        assert_eq!(ipc.account_id, None);
    }

    #[test]
    fn dir_unavailable_maps_to_internal_code() {
        let ipc = to_ipc_error(CoreError::Platform(PlatformError::DirUnavailable(
            "x".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::Internal);
    }

    #[test]
    fn desktop_platform_data_dir_is_wired() {
        let p = DesktopPlatform;
        let dir = p
            .data_dir()
            .expect("data_dir should resolve on the test host");
        assert!(dir.ends_with("dev.tgorka.keeper"));
    }

    #[test]
    fn keychain_error_maps_to_internal_code() {
        let ipc = to_ipc_error(CoreError::Platform(PlatformError::Keychain(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::Internal);
        assert!(!ipc.retriable);
    }

    #[test]
    fn auth_server_unreachable_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::ServerUnreachable(
            "x".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::ServerUnreachable);
        assert!(ipc.retriable, "unreachable server should be retriable");
    }

    #[test]
    fn auth_invalid_credentials_maps_to_non_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::InvalidCredentials));
        assert_eq!(ipc.code, IpcErrorCode::InvalidCredentials);
        assert!(!ipc.retriable);
    }

    #[test]
    fn auth_unsupported_login_type_maps_to_non_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::UnsupportedLoginType(
            "x".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::UnsupportedLoginType);
        assert!(!ipc.retriable);
    }

    #[test]
    fn auth_sliding_sync_unsupported_maps_to_non_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::SlidingSyncUnsupported));
        assert_eq!(ipc.code, IpcErrorCode::SlidingSyncUnsupported);
        assert!(!ipc.retriable);
    }

    #[test]
    fn auth_oauth_unsupported_maps_to_non_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::OAuthUnsupported));
        assert_eq!(ipc.code, IpcErrorCode::OauthUnsupported);
        assert!(!ipc.retriable, "an unsupported server is not retriable");
    }

    #[test]
    fn auth_oauth_timed_out_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::OAuthTimedOut));
        assert_eq!(ipc.code, IpcErrorCode::OauthTimedOut);
        assert!(ipc.retriable, "a timed-out sign-in may be retried");
    }

    #[test]
    fn auth_oauth_cancelled_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::OAuthCancelled));
        assert_eq!(ipc.code, IpcErrorCode::OauthCancelled);
        assert!(ipc.retriable, "a cancelled sign-in may be retried");
    }

    #[test]
    fn auth_oauth_failed_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::OAuthFailed(
            "access_denied".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::OauthFailed);
        assert!(ipc.retriable, "a failed sign-in may be retried");
    }

    #[test]
    fn auth_beeper_unavailable_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::BeeperUnavailable(
            "the Beeper login service returned an error".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::BeeperUnavailable);
        assert!(ipc.retriable, "a Beeper failure may be retried");
    }

    #[test]
    fn account_session_missing_maps_to_retriable_sync_unavailable() {
        let ipc = to_ipc_error(CoreError::Account(AccountError::SessionMissing));
        assert_eq!(ipc.code, IpcErrorCode::SyncUnavailable);
        assert!(ipc.retriable, "sync unavailable should be retriable");
    }

    #[test]
    fn account_restore_failed_maps_to_retriable_sync_unavailable() {
        let ipc = to_ipc_error(CoreError::Account(AccountError::RestoreFailed(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::SyncUnavailable);
        assert!(ipc.retriable);
    }

    #[test]
    fn account_sync_start_maps_to_retriable_sync_unavailable() {
        let ipc = to_ipc_error(CoreError::Account(AccountError::SyncStart(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::SyncUnavailable);
        assert!(ipc.retriable);
    }

    #[test]
    fn timeline_room_not_found_maps_to_retriable_timeline_unavailable() {
        let ipc = to_ipc_error(CoreError::Timeline(TimelineError::RoomNotFound));
        assert_eq!(ipc.code, IpcErrorCode::TimelineUnavailable);
        assert!(ipc.retriable, "timeline unavailable should be retriable");
    }

    #[test]
    fn timeline_build_maps_to_retriable_timeline_unavailable() {
        let ipc = to_ipc_error(CoreError::Timeline(TimelineError::Build("boom".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::TimelineUnavailable);
        assert!(ipc.retriable);
    }

    #[test]
    fn send_room_not_found_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::RoomNotFound));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable, "send failure should be retriable");
    }

    #[test]
    fn send_no_open_timeline_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::NoOpenTimeline));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn send_echo_not_found_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::EchoNotFound));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn send_dispatch_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::Dispatch("boom".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn send_upload_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::Upload("boom".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable, "an enqueue-time upload failure is retriable");
    }

    #[test]
    fn required_header_reads_an_ascii_value() {
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert("x-room-id", "!room:example.org".parse().expect("valid"));
        assert_eq!(
            required_header(&headers, "x-room-id").expect("present"),
            "!room:example.org"
        );
    }

    #[test]
    fn required_header_missing_maps_to_send_failed() {
        let headers = tauri::http::HeaderMap::new();
        let err = required_header(&headers, "x-account-id").expect_err("missing header");
        assert_eq!(err.code, IpcErrorCode::SendFailed);
        assert!(err.retriable);
    }

    #[test]
    fn decode_header_percent_decodes_non_ascii() {
        let mut headers = tauri::http::HeaderMap::new();
        // "café.png" percent-encoded (the caller encodes non-ASCII filenames).
        headers.insert("x-filename", "caf%C3%A9.png".parse().expect("valid"));
        assert_eq!(
            decode_header(&headers, "x-filename"),
            Some("café.png".to_owned())
        );
    }

    #[test]
    fn decode_header_absent_and_empty_are_none() {
        let mut headers = tauri::http::HeaderMap::new();
        assert_eq!(decode_header(&headers, "x-caption"), None);
        headers.insert("x-caption", "".parse().expect("valid"));
        assert_eq!(decode_header(&headers, "x-caption"), None);
    }

    #[test]
    fn send_target_not_found_maps_to_non_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::TargetNotFound));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(
            !ipc.retriable,
            "a missing reply/edit target is not retriable"
        );
    }

    #[test]
    fn send_not_editable_maps_to_non_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::NotEditable));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(!ipc.retriable, "a non-editable message is not retriable");
    }

    #[test]
    fn send_empty_body_maps_to_non_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::EmptyBody));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(
            !ipc.retriable,
            "an empty-draft approve is not retriable (re-issuing empty won't help)"
        );
    }

    #[test]
    fn verification_unavailable_maps_to_retriable_verification_failed() {
        let ipc = to_ipc_error(CoreError::Verification(VerificationError::Unavailable(
            "no identity".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::VerificationFailed);
        assert!(ipc.retriable, "verification failure should be retriable");
    }

    #[test]
    fn verification_flow_not_found_maps_to_retriable_verification_failed() {
        let ipc = to_ipc_error(CoreError::Verification(VerificationError::FlowNotFound));
        assert_eq!(ipc.code, IpcErrorCode::VerificationFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn verification_action_maps_to_retriable_verification_failed() {
        let ipc = to_ipc_error(CoreError::Verification(VerificationError::Action(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::VerificationFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_malformed_key_maps_to_named_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::MalformedRecoveryKey));
        assert_eq!(ipc.code, IpcErrorCode::BackupMalformedKey);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_incorrect_key_maps_to_named_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::IncorrectRecoveryKey));
        assert_eq!(ipc.code, IpcErrorCode::BackupIncorrectKey);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_already_exists_maps_to_backup_exists_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::AlreadyExistsOnServer));
        assert_eq!(ipc.code, IpcErrorCode::BackupExists);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_unavailable_maps_to_backup_failed_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::Unavailable("x".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::BackupFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_restore_failed_maps_to_backup_failed_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::RestoreFailed(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::BackupFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_action_maps_to_backup_failed_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::Action("boom".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::BackupFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn signal_dispatch_maps_to_non_retriable_signal_code() {
        // A best-effort receipt/typing dispatch failure (Story 3.9, AD-14) maps to
        // the named, non-retriable signal code (in practice it is swallowed in the
        // core, so this only keeps the funnel exhaustive).
        let ipc = to_ipc_error(CoreError::Signal(SignalError::Dispatch("boom".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::SignalDispatchFailed);
        assert!(
            !ipc.retriable,
            "a best-effort signal failure is not retriable"
        );
    }

    use chrono::TimeZone;

    // --- the template names the session (Story 40.3) -------------------------
    //
    // `recording_start` itself needs a Tauri `State` and a sidecar, so the part
    // this story changes is exercised through `create_session_folder`, which is
    // the whole naming decision: render, ordinal retry, intermediate creation and
    // unwind. `make` stands in for `SessionManifest::create_with_meta` where a
    // test wants a failure the filesystem will not produce on demand.

    /// A fixed local instant, so a rendered name is a constant a test can assert.
    fn start_at(hour: u32, minute: u32, second: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 8, 6, hour, minute, second)
            .single()
            .expect("an unambiguous local instant")
    }

    fn parsed(template: &str) -> PathTemplate {
        PathTemplate::parse(template).expect("the template parses")
    }

    fn reserved_set() -> Arc<Mutex<HashSet<PathBuf>>> {
        Arc::new(Mutex::new(HashSet::new()))
    }

    /// The real creator, with a session id in the metadata like a start has.
    fn real_make(
        session_id: &str,
    ) -> impl FnMut(PathBuf) -> Result<SessionManifest, RecordingError> {
        let meta = keeper_core::recording::SessionMeta {
            session_id: Some(session_id.to_owned()),
            ..Default::default()
        };
        move |folder| {
            SessionManifest::create_with_meta(
                folder,
                CaptureTarget::display(None),
                SessionDevices {
                    system_audio: true,
                    microphone: false,
                    camera: false,
                },
                Some(meta.clone()),
                Some("2026-08-06T15:36:22+02:00".to_owned()),
            )
        }
    }

    #[test]
    fn the_default_template_nests_and_creates_the_year_on_demand() {
        let root = scan_temp_dir("start-default");
        let (_, folder, _reservation) = create_session_folder(
            &reserved_set(),
            &root,
            &parsed(DEFAULT_TEMPLATE),
            &start_at(15, 36, 22),
            None,
            real_make("01DEVICE-01SESSION"),
        )
        .expect("the session folder is created");
        assert_eq!(folder, root.join("2026").join("2026-08-06 1536"));
        assert!(
            root.join("2026").is_dir(),
            "the year folder is created on demand"
        );
        assert!(folder.join("manifest.json").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_start_lands_exactly_where_the_card_previewed() {
        // The defect this story exists to fix: the field report's saved template
        // previewed one path and the recorder created another. Both sides are
        // asserted against each other here, not against a literal, so they cannot
        // drift apart again without this failing.
        let root = scan_temp_dir("start-preview");
        let template = "{yyyy}/{yyyy}-{mm}-{dd} {HH}.{MM} {slug}";
        let now = start_at(15, 36, 22);
        let preview =
            compose_path_preview(&root, template, &preview_render_ctx(&now, Some("Test")));
        let (_, folder, _reservation) = create_session_folder(
            &reserved_set(),
            &root,
            &parsed(template),
            &now,
            Some("Test"),
            real_make("01DEVICE-01SESSION"),
        )
        .expect("the session folder is created");
        assert_eq!(
            preview.absolute_path.as_deref(),
            Some(folder.to_string_lossy().as_ref()),
            "the preview promised a different path than the start created"
        );
        assert_eq!(folder, root.join("2026").join("2026-08-06 15.36 test"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn two_starts_in_one_minute_get_two_folders() {
        let root = scan_temp_dir("start-collision");
        let reserved = reserved_set();
        let now = start_at(15, 36, 22);
        let (_, first, first_reservation) = create_session_folder(
            &reserved,
            &root,
            &parsed(DEFAULT_TEMPLATE),
            &now,
            Some("Standup"),
            real_make("01DEVICE-01FIRST"),
        )
        .expect("the first session folder");
        // Same minute, same title, same template: only the ordinal can differ.
        let (_, second, _second_reservation) = create_session_folder(
            &reserved,
            &root,
            &parsed(DEFAULT_TEMPLATE),
            &now,
            Some("Standup"),
            real_make("01DEVICE-01SECOND"),
        )
        .expect("the second session folder");
        assert_ne!(first, second, "the second start reused the first folder");
        assert!(first.is_dir() && second.is_dir());
        // Against the renderer, not against a literal or a substring: the story's
        // claim is that the ORDINAL decides the second name, and only asking the
        // template for ordinal 2 can tell that apart from any other disambiguation.
        assert_eq!(
            second,
            session_folder_path(
                &root,
                &parsed(DEFAULT_TEMPLATE).render(&start_render_ctx(&now, Some("Standup"), 2))
            ),
            "the second folder is the template's ordinal-2 render"
        );
        // The colliding attempt reserved the FIRST session's folder before it
        // discovered the collision. Releasing that attempt's guard must not
        // release the live session's reservation, or the recovery pass would
        // rewrite a recording session's manifest underneath it.
        assert!(
            plain_lock(&reserved).contains(&first),
            "the live session's folder was un-reserved by a colliding attempt"
        );
        drop(first_reservation);
        assert!(
            !plain_lock(&reserved).contains(&first),
            "the owning guard did not release the reservation"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_exhausted_ordinal_refuses_and_names_the_path_it_tried() {
        let root = scan_temp_dir("start-exhausted");
        let template = parsed("rec{seq}");
        let now = start_at(15, 36, 22);
        // Occupy every ordinal this start would try.
        for seq in 1..=SESSION_FOLDER_ATTEMPTS {
            let taken = template.render(&start_render_ctx(&now, None, seq));
            std::fs::create_dir_all(session_folder_path(&root, &taken)).expect("occupy");
        }
        let error = create_session_folder(
            &reserved_set(),
            &root,
            &template,
            &now,
            None,
            real_make("01DEVICE-01SESSION"),
        )
        .expect_err("every ordinal is taken");
        let last = template.render(&start_render_ctx(&now, None, SESSION_FOLDER_ATTEMPTS));
        assert!(
            error.message.contains(last.as_str()),
            "the refusal must name the path it tried, got: {}",
            error.message
        );
        assert!(
            !error.retriable,
            "retrying the same 64 ordinals cannot help"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_root_refuses_and_leaves_nothing_behind() {
        use std::os::unix::fs::PermissionsExt;
        let root = scan_temp_dir("start-readonly");
        let mut perms = std::fs::metadata(&root)
            .expect("root metadata")
            .permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&root, perms).expect("make the root read-only");
        let outcome = create_session_folder(
            &reserved_set(),
            &root,
            &parsed(DEFAULT_TEMPLATE),
            &start_at(15, 36, 22),
            None,
            real_make("01DEVICE-01SESSION"),
        );
        // Restore BEFORE asserting: a failed assertion here would otherwise
        // unwind past the restore and strand an unwritable directory in the temp
        // root that no later run can clean up.
        let mut restore = std::fs::metadata(&root)
            .expect("root metadata")
            .permissions();
        restore.set_mode(0o755);
        std::fs::set_permissions(&root, restore).expect("restore the root");
        let error = outcome.expect_err("a read-only root cannot hold a session");
        assert!(
            error.message.contains("2026/2026-08-06 1536"),
            "the refusal must name the rendered path, got: {}",
            error.message
        );
        assert!(
            !root.join("2026").exists(),
            "a refused start left an intermediate directory behind"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_creation_unwinds_the_directories_it_made() {
        let root = scan_temp_dir("start-unwind");
        let error = create_session_folder(
            &reserved_set(),
            &root,
            &parsed(DEFAULT_TEMPLATE),
            &start_at(15, 36, 22),
            None,
            // The real creator's shape when the manifest write fails: the leaf
            // exists, its manifest does not.
            |folder: PathBuf| {
                std::fs::create_dir(&folder).expect("the leaf is creatable");
                Err(RecordingError::ManifestIo(
                    "write manifest: disk lied".to_owned(),
                ))
            },
        )
        .expect_err("the manifest write failed");
        assert!(error.message.contains("disk lied"));
        assert!(
            !root.join("2026").join("2026-08-06 1536").exists(),
            "the leaf this attempt created was left behind"
        );
        assert!(
            !root.join("2026").exists(),
            "the year folder this attempt created was left behind"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_pre_existing_parent_is_never_unwound() {
        let root = scan_temp_dir("start-keep-parent");
        // Two shapes of "this attempt did not create it": a year folder holding
        // someone else's session, and an EMPTY one. The empty case is the one
        // that matters — a non-empty directory is protected by `remove_dir`'s own
        // ENOTEMPTY, so only the empty one can tell "never registered" apart from
        // "registered and got lucky".
        let occupied = root.join("2026");
        std::fs::create_dir_all(occupied.join("an older session")).expect("older session");
        let empty_root = scan_temp_dir("start-keep-empty-parent");
        let empty_year = empty_root.join("2026");
        std::fs::create_dir(&empty_year).expect("an empty year folder");
        for (root, year) in [(&root, &occupied), (&empty_root, &empty_year)] {
            let error = create_session_folder(
                &reserved_set(),
                root,
                &parsed(DEFAULT_TEMPLATE),
                &start_at(15, 36, 22),
                None,
                |_folder: PathBuf| {
                    Err(RecordingError::ManifestIo(
                        "write manifest: nope".to_owned(),
                    ))
                },
            )
            .expect_err("the manifest write failed");
            assert!(error.message.contains("nope"));
            assert!(
                year.is_dir(),
                "a directory this attempt did not create must never be removed: {year:?}"
            );
        }
        assert!(
            occupied.join("an older session").is_dir(),
            "another session's folder must survive a failed start"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&empty_root);
    }

    #[test]
    fn the_manifest_carries_the_session_id_and_never_the_destination_root() {
        let root = scan_temp_dir("start-manifest");
        let (_, folder, _reservation) = create_session_folder(
            &reserved_set(),
            &root,
            &parsed(DEFAULT_TEMPLATE),
            &start_at(15, 36, 22),
            Some("Standup"),
            real_make("01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KZAAAAAAAAAAAAAAAAAAAAAA"),
        )
        .expect("the session folder is created");
        let text = std::fs::read_to_string(folder.join("manifest.json")).expect("manifest text");
        assert!(
            text.contains(
                "\"sessionId\": \"01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KZAAAAAAAAAAAAAAAAAAAAAA\""
            ),
            "the manifest must carry the session identity: {text}"
        );
        assert!(
            !text.contains(root.to_string_lossy().as_ref()),
            "the manifest must stay portable — no absolute path: {text}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(desktop)]
    #[test]
    fn the_session_id_is_device_scoped_and_never_repeats() {
        let data_dir = scan_temp_dir("start-identity");
        let first = mint_session_id(&data_dir).expect("mint a session id");
        let second = mint_session_id(&data_dir).expect("mint another session id");
        let (first_device, first_session) = first
            .split_once('-')
            .expect("the id splits into device and session");
        let (second_device, second_session) = second
            .split_once('-')
            .expect("the id splits into device and session");
        assert_eq!(
            first_device, second_device,
            "the device half is this machine's identity and does not move"
        );
        assert_ne!(
            first_session, second_session,
            "each session gets its own ULID"
        );
        assert_eq!(first_device.len(), 26, "a ULID is 26 Crockford characters");
        assert_eq!(first_session.len(), 26);
        assert!(
            first.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "the id is used in a markdown link and a shell word: {first}"
        );
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    // --- a retitle moves the folder, not the identity (Story 40.4) -----------
    //
    // `recording_retitle` needs a Tauri `State`, a registry and a sync engine, so
    // the part this story decides is exercised through `retitle_session_folder`:
    // the live claim, the re-render at the session's OWN instant, the ordinal
    // walk, the `create_dir` + `rename` move, the manifest rewrite and the
    // scaffold unwind.

    /// A fixed local instant on an arbitrary day, for the sessions whose start is
    /// not the one [`start_at`] pins.
    fn local_at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .expect("an unambiguous local instant")
    }

    /// A creator that stamps the manifest with `started_at` the way
    /// `recording_start` does, so a retitle re-renders against a KNOWN instant
    /// whatever zone the test machine is in.
    fn make_started_at(
        session_id: &str,
        started_at: &DateTime<Local>,
    ) -> impl FnMut(PathBuf) -> Result<SessionManifest, RecordingError> {
        let meta = keeper_core::recording::SessionMeta {
            session_id: Some(session_id.to_owned()),
            ..Default::default()
        };
        let stamp = started_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        move |folder| {
            SessionManifest::create_with_meta(
                folder,
                CaptureTarget::display(None),
                SessionDevices {
                    system_audio: true,
                    microphone: false,
                    camera: false,
                },
                Some(meta.clone()),
                Some(stamp.clone()),
            )
        }
    }

    /// Put a finished session on disk the way a completed recording leaves one:
    /// named from `named_at`, stamped `started_at`, titled, with one 100-byte
    /// screen segment in its ledger (the file that must ride along on a move).
    fn seed_session(
        root: &Path,
        template: &PathTemplate,
        named_at: &DateTime<Local>,
        started_at: &DateTime<Local>,
        title: Option<&str>,
        session_id: &str,
    ) -> PathBuf {
        let (mut manifest, folder, _reservation) = create_session_folder(
            &reserved_set(),
            root,
            template,
            named_at,
            title,
            make_started_at(session_id, started_at),
        )
        .expect("the session folder is created");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 100]).expect("segment");
        manifest.reconcile_from_dir().expect("reconcile");
        manifest.retitle(title.map(str::to_owned));
        manifest.set_status(ManifestStatus::Finalized);
        manifest.write().expect("write manifest");
        folder
    }

    /// The session id every retitle test asserts is byte-identical afterwards.
    const RETITLE_ID: &str = "01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KZAAAAAAAAAAAAAAAAAAAAAA";

    #[cfg(desktop)]
    #[test]
    fn a_retitle_moves_the_folder_and_never_the_identity() {
        let root = scan_temp_dir("retitle-identity");
        let template = parsed(DEFAULT_TEMPLATE);
        let at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &at, &at, None, RETITLE_ID);
        let (manifest, moved) =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Standup"))
                .expect("the retitle");
        // Against the renderer, not a literal: the story's claim is that the
        // TEMPLATE decides where a retitled session lands.
        assert_eq!(
            moved,
            session_folder_path(
                &root,
                &template.render(&start_render_ctx(&at, Some("Standup"), 1))
            )
        );
        assert_eq!(moved, root.join("2026").join("2026-08-05 1432 standup"));
        assert!(!folder.exists(), "the session was left in its old folder");
        assert!(
            moved.join("screen-0000.mov").is_file(),
            "the media did not ride along"
        );
        assert_eq!(
            manifest.folder(),
            moved,
            "the manifest still points at the old folder"
        );
        // Byte-identical, read back off disk: the identity is the one thing a
        // retitle may not touch.
        let text = std::fs::read_to_string(moved.join("manifest.json")).expect("manifest text");
        assert!(
            text.contains(&format!("\"sessionId\": \"{RETITLE_ID}\"")),
            "the identity moved with the folder: {text}"
        );
        assert_eq!(
            manifest
                .meta
                .as_ref()
                .and_then(|meta| meta.title.as_deref()),
            Some("Standup")
        );
        // The `session` label follows the folder — it is a label, and a retitle is
        // the one place it is allowed to change.
        assert_eq!(
            Some(manifest.session.as_str()),
            moved.file_name().and_then(|name| name.to_str())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(desktop)]
    #[test]
    fn naming_renaming_and_clearing_land_on_the_rendered_paths() {
        let root = scan_temp_dir("retitle-titles");
        let template = parsed(DEFAULT_TEMPLATE);
        let at = local_at(2026, 8, 5, 14, 32);
        let untitled = seed_session(&root, &template, &at, &at, None, RETITLE_ID);
        assert_eq!(untitled, root.join("2026").join("2026-08-05 1432"));
        let rendered = |title: Option<&str>| {
            session_folder_path(&root, &template.render(&start_render_ctx(&at, title, 1)))
        };

        let (_, named) = retitle_session_folder(
            &reserved_set(),
            &root,
            &template,
            &untitled,
            Some("Standup"),
        )
        .expect("name an untitled session");
        assert_eq!(named, rendered(Some("Standup")));
        assert_eq!(named, root.join("2026").join("2026-08-05 1432 standup"));

        let (_, renamed) =
            retitle_session_folder(&reserved_set(), &root, &template, &named, Some("Retro"))
                .expect("rename a titled session");
        assert_eq!(renamed, rendered(Some("Retro")));
        assert!(
            renamed.join("screen-0000.mov").is_file(),
            "the media did not ride along the second move"
        );

        // Clearing moves it back to the name an untitled session has, and leaves
        // no title in the manifest at all.
        let (cleared_manifest, cleared) =
            retitle_session_folder(&reserved_set(), &root, &template, &renamed, Some(""))
                .expect("clear the title");
        assert_eq!(cleared, rendered(None));
        assert_eq!(
            cleared, untitled,
            "a cleared title renders the untitled name"
        );
        assert_eq!(
            cleared_manifest
                .meta
                .as_ref()
                .and_then(|meta| meta.title.as_deref()),
            None
        );
        let text = std::fs::read_to_string(cleared.join("manifest.json")).expect("manifest text");
        assert!(
            !text.contains("\"title\""),
            "a cleared title must not be serialized at all: {text}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(desktop)]
    #[test]
    fn a_retitle_that_renders_the_current_folder_moves_nothing_and_still_retitles() {
        let root = scan_temp_dir("retitle-in-place");
        // No `{slug}`/`{title}` anywhere in this template, so every title renders
        // the folder the session already occupies.
        let template = parsed("{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM}");
        let at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &at, &at, Some("Standup"), RETITLE_ID);
        let (manifest, moved) =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Retro"))
                .expect("the retitle");
        assert_eq!(moved, folder, "nothing renders elsewhere, so nothing moves");
        assert!(folder.join("screen-0000.mov").is_file());
        assert_eq!(
            manifest
                .meta
                .as_ref()
                .and_then(|meta| meta.title.as_deref()),
            Some("Retro"),
            "the title must be rewritten even when the folder does not move"
        );
        let text = std::fs::read_to_string(folder.join("manifest.json")).expect("manifest text");
        assert!(
            text.contains("\"title\": \"Retro\""),
            "the rewritten title never reached disk: {text}"
        );
        assert_eq!(
            std::fs::read_dir(root.join("2026"))
                .expect("read the year")
                .count(),
            1,
            "a retitle that moves nothing created a second folder"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(desktop)]
    #[test]
    fn a_colliding_retitle_takes_the_next_ordinal_and_leaves_the_occupant_alone() {
        let root = scan_temp_dir("retitle-collision");
        let template = parsed(DEFAULT_TEMPLATE);
        let at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &at, &at, None, RETITLE_ID);
        // Ordinal 1 for the new title is already somebody else's folder.
        let occupied = session_folder_path(
            &root,
            &template.render(&start_render_ctx(&at, Some("Standup"), 1)),
        );
        std::fs::create_dir_all(&occupied).expect("occupy ordinal 1");
        std::fs::write(occupied.join("keep.txt"), b"not yours").expect("marker");
        let (_, moved) =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Standup"))
                .expect("the retitle");
        assert_eq!(
            moved,
            session_folder_path(
                &root,
                &template.render(&start_render_ctx(&at, Some("Standup"), 2))
            ),
            "the move must take the template's ordinal 2"
        );
        assert_eq!(
            std::fs::read_to_string(occupied.join("keep.txt")).expect("marker"),
            "not yours",
            "the occupying folder was written into"
        );
        assert_eq!(
            std::fs::read_dir(&occupied)
                .expect("read the occupant")
                .count(),
            1,
            "the session was moved inside the occupied folder"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(desktop)]
    #[test]
    fn a_live_session_is_refused_and_not_one_byte_moves() {
        let root = scan_temp_dir("retitle-live");
        let template = parsed(DEFAULT_TEMPLATE);
        let at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &at, &at, None, RETITLE_ID);
        let before = std::fs::read_to_string(folder.join("manifest.json")).expect("manifest text");
        // Exactly what a live (or starting) session holds — the same set the
        // recovery pass's `is_active` predicate reads.
        let reserved = reserved_set();
        plain_lock(&reserved).insert(folder.clone());
        let error = retitle_session_folder(&reserved, &root, &template, &folder, Some("Standup"))
            .expect_err("a recording session cannot be renamed");
        assert_eq!(error.code, IpcErrorCode::RecordingSessionLive);
        assert!(
            !error.retriable,
            "nothing clears while the session is still recording"
        );
        assert!(folder.is_dir(), "the live session's folder moved");
        assert_eq!(
            std::fs::read_to_string(folder.join("manifest.json")).expect("manifest text"),
            before,
            "the live session's manifest was rewritten"
        );
        assert!(
            !session_folder_path(
                &root,
                &template.render(&start_render_ctx(&at, Some("Standup"), 1))
            )
            .exists(),
            "the refused retitle created its destination anyway"
        );
        // The refusal must not release the reservation it found: that entry
        // belongs to the live session, and dropping it would let the recovery
        // pass rewrite a recording manifest.
        assert!(
            plain_lock(&reserved).contains(&folder),
            "the refused retitle un-reserved a live session"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(desktop)]
    #[test]
    fn a_folder_with_no_manifest_is_not_a_session() {
        let root = scan_temp_dir("retitle-not-a-session");
        let template = parsed(DEFAULT_TEMPLATE);
        let folder = root.join("2026").join("just a folder");
        std::fs::create_dir_all(&folder).expect("a folder that is not a session");
        let error =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Standup"))
                .expect_err("a folder without a manifest is not a session");
        assert!(
            error.message.contains("session manifest"),
            "the refusal must say what it could not read, got: {}",
            error.message
        );
        assert!(folder.is_dir(), "the folder was moved anyway");
        assert_eq!(
            std::fs::read_dir(root.join("2026"))
                .expect("read the year")
                .count(),
            1,
            "the refusal created something under the root"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(desktop)]
    #[test]
    fn an_exhausted_retitle_refuses_and_names_the_path_it_tried() {
        let root = scan_temp_dir("retitle-exhausted");
        let template = parsed(DEFAULT_TEMPLATE);
        let at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &at, &at, None, RETITLE_ID);
        for seq in 1..=SESSION_FOLDER_ATTEMPTS {
            let taken = template.render(&start_render_ctx(&at, Some("Standup"), seq));
            std::fs::create_dir_all(session_folder_path(&root, &taken)).expect("occupy");
        }
        let error =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Standup"))
                .expect_err("every ordinal is taken");
        let last = template.render(&start_render_ctx(
            &at,
            Some("Standup"),
            SESSION_FOLDER_ATTEMPTS,
        ));
        assert!(
            error.message.contains(last.as_str()),
            "the refusal must name the path it tried, got: {}",
            error.message
        );
        assert!(
            !error.retriable,
            "retrying the same 64 ordinals cannot help"
        );
        assert!(
            folder.join("manifest.json").is_file(),
            "the session must stay exactly where it is"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(all(unix, desktop))]
    #[test]
    fn a_failed_move_unwinds_the_directories_it_made() {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        let root = scan_temp_dir("retitle-unwind");
        // Root ignores a directory's write bit, so the read-only parent below
        // would not stop `rename` and the test would hard-fail on the
        // environment rather than on the behaviour it defends (a root container
        // is a common CI default once this crate links on Linux). The owner of a
        // file this process just created IS this process's effective uid, so no
        // new dependency is needed to tell; an unreadable root is not root, and
        // running the test is the safe reading of that.
        if std::fs::metadata(&root).is_ok_and(|metadata| metadata.uid() == 0) {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        // The title names the PARENT here, so the destination's intermediate is one
        // this attempt must create while the source sits in a DIFFERENT directory —
        // which is what makes the rename, rather than the create, the failure: a
        // rename has to unlink the entry from the source's parent.
        let template = parsed("{slug}-rec/{yyyy}-{mm}-{dd} {HH}{MM}");
        let at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &at, &at, Some("One"), RETITLE_ID);
        assert_eq!(folder, root.join("one-rec").join("2026-08-05 1432"));
        let source_parent = root.join("one-rec");
        let mut perms = std::fs::metadata(&source_parent)
            .expect("parent metadata")
            .permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&source_parent, perms)
            .expect("make the source's parent read-only");
        let outcome =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Two"));
        // Restore BEFORE asserting: a failed assertion here would otherwise unwind
        // past the restore and strand an unwritable directory in the temp root that
        // no later run can clean up.
        let mut restore = std::fs::metadata(&source_parent)
            .expect("parent metadata")
            .permissions();
        restore.set_mode(0o755);
        std::fs::set_permissions(&source_parent, restore).expect("restore the parent");
        let error = outcome.expect_err("the rename cannot unlink from a read-only parent");
        assert!(
            error.message.contains("two-rec/2026-08-05 1432"),
            "the refusal must name the rendered path, got: {}",
            error.message
        );
        assert!(
            !root.join("two-rec").exists(),
            "the intermediates this attempt created were left behind"
        );
        assert!(
            folder.join("manifest.json").is_file(),
            "the session must stay exactly where it is"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(desktop)]
    #[test]
    fn the_retitle_renders_against_the_sessions_own_start_instant() {
        let root = scan_temp_dir("retitle-own-instant");
        let template = parsed(DEFAULT_TEMPLATE);
        // The folder was named in August; the manifest says the session started in
        // May. A session renamed today must not migrate into today's folder.
        let named_at = local_at(2026, 8, 5, 14, 32);
        let started_at = local_at(2026, 5, 2, 9, 10);
        let folder = seed_session(&root, &template, &named_at, &started_at, None, RETITLE_ID);
        assert_eq!(folder, root.join("2026").join("2026-08-05 1432"));
        let (_, moved) =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Standup"))
                .expect("the retitle");
        assert_eq!(
            moved,
            session_folder_path(
                &root,
                &template.render(&start_render_ctx(&started_at, Some("Standup"), 1))
            ),
            "the re-render must use the manifest's instant, not the clock"
        );
        assert_eq!(moved, root.join("2026").join("2026-05-02 0910 standup"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `unix` because the SETUP is not portable, not the behaviour: pushing a
    /// DIRECTORY's mtime into the past needs `File::open` on that directory,
    /// which `futimens` accepts for the owner on ext4 and APFS alike but which
    /// Windows refuses outright (a directory handle needs
    /// `FILE_FLAG_BACKUP_SEMANTICS`, and `set_times` there wants write access).
    /// The same `unix` gate the sibling unwind test carries.
    #[cfg(all(unix, desktop))]
    #[test]
    fn a_session_with_no_start_stamp_falls_back_to_the_folders_modification_time() {
        let root = scan_temp_dir("retitle-no-stamp");
        let template = parsed(DEFAULT_TEMPLATE);
        let at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &at, &at, None, RETITLE_ID);
        // A pre-Story-21.5 manifest: no `startedAt` at all. It must still retitle,
        // and it must land where the folder's own mtime renders — never `now`.
        let mut manifest = SessionManifest::load(&folder).expect("load the seeded manifest");
        manifest.started_at = None;
        manifest.write().expect("write the stampless manifest");
        // The write above set the folder's mtime to ~now, which is precisely the
        // value a `Local::now()` fallback would produce — so the mtime is pushed
        // into a DIFFERENT month before the retitle runs. Without this the test
        // passes identically against the fallback the spec forbids.
        let modified = local_at(2026, 5, 2, 9, 10);
        std::fs::File::open(&folder)
            .expect("open the session folder")
            .set_times(
                std::fs::FileTimes::new()
                    .set_accessed(SystemTime::from(modified))
                    .set_modified(SystemTime::from(modified)),
            )
            .expect("push the folder's mtime into the past");
        let (_, moved) =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Standup"))
                .expect("a stampless session still retitles");
        // A literal, like its sibling: rendering the assertion through the same
        // `start_render_ctx` the implementation uses would pin nothing.
        assert_eq!(
            moved,
            root.join("2026").join("2026-05-02 0910 standup"),
            "the fallback must be the folder's modification time"
        );
        let now = Local::now();
        assert_ne!(
            moved,
            session_folder_path(
                &root,
                &template.render(&start_render_ctx(&now, Some("Standup"), 1))
            ),
            "the fallback landed the session in the CURRENT month, i.e. it read the clock"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Story 40.4: the re-render must read the six civil fields off the OFFSET
    /// the stamp carries, not off the machine's current zone. Every other retitle
    /// test builds its stamp from `Local`, so all of them round-trip by
    /// construction and none can see this; the stamp here is built with an
    /// explicit `FixedOffset` for exactly that reason.
    #[cfg(desktop)]
    #[test]
    fn the_re_render_uses_the_stamps_own_offset_not_the_machines_zone() {
        let root = scan_temp_dir("retitle-stamp-offset");
        let template = parsed(DEFAULT_TEMPLATE);
        let named_at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &named_at, &named_at, None, RETITLE_ID);
        // 00:30 on New Year's Day at +14:00 — the easternmost offset in use, so
        // every other zone reads this instant as the PREVIOUS year (UTC sees
        // 2025-12-31 1030). Converting to `Local` first therefore renders a
        // different year folder on any machine east of nowhere, which is the
        // migration this pins shut.
        let stamped = FixedOffset::east_opt(14 * 3600)
            .expect("+14:00")
            .with_ymd_and_hms(2026, 1, 1, 0, 30, 0)
            .single()
            .expect("an unambiguous instant");
        let mut manifest = SessionManifest::load(&folder).expect("load the seeded manifest");
        manifest.started_at = Some(stamped.to_rfc3339_opts(chrono::SecondsFormat::Secs, false));
        manifest.write().expect("write the re-stamped manifest");
        let (_, moved) =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("Standup"))
                .expect("the retitle");
        assert_eq!(
            moved,
            root.join("2026").join("2026-01-01 0030 standup"),
            "the re-render must use the stamp's own offset, not this machine's zone"
        );
        assert!(
            !root.join("2025").exists(),
            "the retitle re-rendered in the machine's zone and migrated the session across a year"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Story 40.4: on a volume that folds case, `…1432 Standup` and
    /// `…1432 standup` are the SAME directory, so a case-only retitle must take
    /// the in-place branch. A byte comparison of the two `PathBuf`s does not,
    /// and `create_dir` then collides with the session's own folder and hands it
    /// a permanent ` (2)` suffix.
    ///
    /// Adapted rather than skipped where the filesystem is case-sensitive (this
    /// repo's Linux CI, ext4): there the render is a genuinely different folder
    /// and the move is correct, so that branch asserts the move. Both branches
    /// assert the ordinal was NOT bumped, which is the defect either way.
    #[cfg(desktop)]
    #[test]
    fn a_case_only_retitle_never_takes_an_ordinal() {
        let root = scan_temp_dir("retitle-case-only");
        // `{title}` preserves case where `{slug}` folds it, so this is the only
        // template shape that can render a case-only change at all.
        let template = parsed("{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {title}");
        let at = local_at(2026, 8, 5, 14, 32);
        let folder = seed_session(&root, &template, &at, &at, Some("Standup"), RETITLE_ID);
        assert_eq!(folder, root.join("2026").join("2026-08-05 1432 Standup"));
        let folds_case = root.join("2026").join("2026-08-05 1432 STANDUP").is_dir();
        let (manifest, moved) =
            retitle_session_folder(&reserved_set(), &root, &template, &folder, Some("standup"))
                .expect("the retitle");
        if folds_case {
            assert_eq!(
                moved, folder,
                "a case-only retitle must not move the folder"
            );
        } else {
            assert_eq!(
                moved,
                root.join("2026").join("2026-08-05 1432 standup"),
                "on a case-sensitive volume the render IS a different folder"
            );
        }
        assert!(
            !root
                .join("2026")
                .join("2026-08-05 1432 standup (2)")
                .exists(),
            "the retitle collided with the session's own folder and took an ordinal"
        );
        assert_eq!(
            std::fs::read_dir(root.join("2026"))
                .expect("read the year")
                .count(),
            1,
            "a case-only retitle left a second session folder behind"
        );
        assert_eq!(
            manifest
                .meta
                .as_ref()
                .and_then(|meta| meta.title.as_deref()),
            Some("standup"),
            "the new title must be stored even when nothing moves"
        );
        // The label names the directory that actually exists, whichever branch ran.
        assert_eq!(
            Some(manifest.session.as_str()),
            moved.file_name().and_then(|name| name.to_str())
        );
        assert!(moved.join("screen-0000.mov").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Story 40.4: a claim that stays on the vacated source would un-reserve a
    /// start that reoccupies the name the moment the retitle's guard drops.
    #[test]
    fn a_repointed_claim_releases_the_path_it_left() {
        let reserved = reserved_set();
        let source = PathBuf::from("/tmp/keeper-claim/source");
        let destination = PathBuf::from("/tmp/keeper-claim/destination");
        let mut claim = LiveFolderReservation::reserve(&reserved, source.clone());
        assert!(claim.owned);
        claim.repoint(destination.clone());
        assert!(
            claim.owned,
            "the destination was free, so this guard owns it"
        );
        assert!(
            !plain_lock(&reserved).contains(&source),
            "the vacated source is still reserved"
        );
        assert!(
            plain_lock(&reserved).contains(&destination),
            "the destination the session moved to is not reserved"
        );
        drop(claim);
        assert!(
            plain_lock(&reserved).is_empty(),
            "dropping the repointed guard must release the destination, not the source"
        );

        // A guard that never owned its entry still must not remove someone
        // else's on the way past — repointing only ever releases what it held.
        let held = LiveFolderReservation::reserve(&reserved, source.clone());
        let mut borrowed = LiveFolderReservation::reserve(&reserved, source.clone());
        assert!(!borrowed.owned);
        borrowed.repoint(destination.clone());
        assert!(
            plain_lock(&reserved).contains(&source),
            "repointing a non-owning guard un-reserved the holder's folder"
        );
        drop(borrowed);
        drop(held);
        assert!(plain_lock(&reserved).is_empty());
    }

    /// Story 40.4: the lexical `strip_prefix` behind the "inside the destination
    /// root" guard preserves `..`, so a folder OUTSIDE the root would strip to a
    /// non-empty relative path and pass a `is_some()` containment check — and the
    /// retitle would then rename it INTO the root.
    #[cfg(desktop)]
    #[test]
    fn a_relative_key_refuses_a_path_that_climbs_out_of_the_root() {
        let root = Path::new("/tmp/keeper-root");
        assert_eq!(
            session_relative_key(root, &root.join("2026").join("session")),
            Some("2026/session".to_owned())
        );
        assert_eq!(
            session_relative_key(root, &root.join("..").join("..").join("elsewhere")),
            None,
            "a ..-bearing path is not inside the root, however it strips"
        );
        assert_eq!(
            session_relative_key(root, &root.join("2026").join("..").join("session")),
            None,
            "a .. anywhere in the path defeats the containment check"
        );
        assert_eq!(
            session_relative_key(root, &root.join(".").join("session")),
            Some("session".to_owned()),
            "a leading `.` is dropped by `components`, so it keys the same session"
        );
        assert_eq!(session_relative_key(root, root), None);
    }

    /// Story 40.4: the kept status snapshot is what the frontend re-adopts on
    /// every remount, so a retitle that leaves it naming the old folder hands the
    /// card a path that no longer exists.
    #[cfg(desktop)]
    #[test]
    fn a_retitle_repoints_the_kept_status_snapshot_only_on_an_exact_match() {
        let slot = run_slot_in(RecordingUiState::Finalized, None);
        let source = PathBuf::from("/tmp/keeper-rec/2026-08-05 1432");
        let destination = PathBuf::from("/tmp/keeper-rec/2026-08-05 1432 standup");
        status_lock(&slot_lock(&slot).as_ref().expect("the slot").status.clone()).output_path =
            Some(source.to_string_lossy().into_owned());

        // A slot describing a DIFFERENT session is none of this retitle's
        // business, even though it is in the same root.
        assert!(!repoint_recording_slot_output(
            &slot,
            Path::new("/tmp/keeper-rec/2026-08-05 1500"),
            &destination
        ));
        assert!(repoint_recording_slot_output(&slot, &source, &destination));
        let snapshot = live_snapshot(&slot).expect("the slot").0;
        assert_eq!(
            snapshot.output_path.as_deref(),
            Some(destination.to_string_lossy().as_ref()),
            "the snapshot still names the folder the session moved out of"
        );

        // An empty slot has nothing to follow, and says so rather than panicking.
        let empty: Mutex<Option<RecordingRun>> = Mutex::new(None);
        assert!(!repoint_recording_slot_output(
            &empty,
            &source,
            &destination
        ));
    }

    // --- Story 41.5: committed at close, pushed on policy -------------------

    /// One call a session made on the engine seam, kept in the order it was made
    /// so a test can assert both the COUNT and what came last.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum SyncCall {
        PushPolicy,
        EnsureLfsRule(String),
        NoteFinished(PathBuf),
        Push(RecordingPushTrigger),
        /// Story 41.6: the status poll asking about a path. Recorded like every
        /// other call so "a plain folder asks the engine nothing" stays one
        /// assertion over one vector.
        Durability(PathBuf),
    }

    /// A counting [`RecordingSyncPort`] double — the test seam this story needs.
    ///
    /// The whole acceptance criteria is stated in counts (48 commits, ONE
    /// `.gitattributes` write, ONE `manifest.json` write, one push), and every one
    /// of them is a call the sink either makes or does not. A real `Engine` would
    /// answer the same questions with a git repository, a `sync.db` and a remote
    /// attached; this answers them with a vector.
    ///
    /// Story 41.6 adds a SCRIPT: the durability answers a session's polls get, in
    /// order. That is what turns the story's I/O matrix into ordinary unit tests —
    /// a protected-branch rejection, a network killed mid-session and a transient
    /// read failure are all just the next row of a `Vec`, and none of them needs a
    /// remote to refuse anything.
    struct CountingSyncPort {
        policy: SessionPushPolicy,
        /// An engine that says no to everything: the LFS rule cannot be written
        /// and every assertion is dropped. The recorder must not be able to tell.
        refuses: bool,
        calls: Mutex<Vec<SyncCall>>,
        /// The scripted durability answers, one consumed per ask. The LAST row
        /// repeats forever, because a script describes the CHANGES and a ~1 Hz
        /// poll keeps asking long after the last one.
        durability: Mutex<Vec<Result<SegmentDurability, String>>>,
    }

    impl CountingSyncPort {
        fn new(policy: SessionPushPolicy) -> Self {
            Self {
                policy,
                refuses: false,
                calls: Mutex::new(Vec::new()),
                durability: Mutex::new(Vec::new()),
            }
        }

        /// A refusing engine reports the default policy, because that is what a
        /// profile it cannot read degrades to.
        fn refusing() -> Self {
            Self {
                policy: SessionPushPolicy::default(),
                refuses: true,
                calls: Mutex::new(Vec::new()),
                durability: Mutex::new(Vec::new()),
            }
        }

        /// A port whose durability answers are these, in order.
        fn scripted(answers: Vec<Result<SegmentDurability, String>>) -> Self {
            Self {
                policy: SessionPushPolicy::default(),
                refuses: false,
                calls: Mutex::new(Vec::new()),
                durability: Mutex::new(answers),
            }
        }

        fn record(&self, call: SyncCall) {
            self.calls.lock().expect("lock sync calls").push(call);
        }

        fn calls(&self) -> Vec<SyncCall> {
            self.calls.lock().expect("lock sync calls").clone()
        }

        fn count(&self, want: &SyncCall) -> usize {
            self.calls().iter().filter(|call| *call == want).count()
        }

        fn assertions(&self) -> usize {
            self.calls()
                .iter()
                .filter(|call| matches!(call, SyncCall::NoteFinished(_)))
                .count()
        }

        fn pushes(&self, trigger: RecordingPushTrigger) -> usize {
            self.count(&SyncCall::Push(trigger))
        }

        /// How many times the status poll asked about durability.
        fn durability_asks(&self) -> usize {
            self.calls()
                .iter()
                .filter(|call| matches!(call, SyncCall::Durability(_)))
                .count()
        }
    }

    impl RecordingSyncPort for CountingSyncPort {
        fn push_policy(&self, _profile_id: &str) -> SessionPushPolicy {
            self.record(SyncCall::PushPolicy);
            self.policy
        }

        fn ensure_lfs_rule(&self, _profile_id: &str, extension: &str) -> Result<bool, String> {
            self.record(SyncCall::EnsureLfsRule(extension.to_owned()));
            if self.refuses {
                Err("this profile has no working tree".to_owned())
            } else {
                Ok(true)
            }
        }

        fn note_finished(&self, _profile_id: &str, path: &Path) -> bool {
            self.record(SyncCall::NoteFinished(path.to_path_buf()));
            !self.refuses
        }

        fn request_push(&self, _profile_id: &str, trigger: RecordingPushTrigger) {
            self.record(SyncCall::Push(trigger));
        }

        fn path_durability(
            &self,
            _profile_id: &str,
            path: &Path,
        ) -> Result<SegmentDurability, String> {
            self.record(SyncCall::Durability(path.to_path_buf()));
            let mut script = self.durability.lock().expect("lock durability script");
            if script.len() > 1 {
                script.remove(0)
            } else {
                // An empty script is an engine that knows nothing yet — which is
                // the honest answer before the first commit, and what every
                // Story 41.5 test (which never polls) would get.
                script
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Ok(SegmentDurability::default()))
            }
        }
    }

    /// A destination that resolved to a recordings-flagged profile whose
    /// recordings root is `root`.
    ///
    /// `volume: None` is the Story 41.7 answer for "a synced folder on a disk
    /// that is always there": removability IS the `Option`, so a fixture that
    /// says `None` is asserting the non-removable case rather than leaving a
    /// field unset. The removable cases build their own destinations.
    fn profile_destination(root: &Path) -> RecordingDestination {
        RecordingDestination {
            root: root.to_path_buf(),
            kind: RecordingDestinationKind::Profile,
            profile_id: Some("profile-1".to_owned()),
            profile_name: Some("tgdrive".to_owned()),
            volume: None,
        }
    }

    /// A plain-folder destination — the same root, no profile behind it, and so
    /// no volume to be attached or not (Story 41.7).
    fn folder_destination(root: &Path) -> RecordingDestination {
        RecordingDestination {
            root: root.to_path_buf(),
            kind: RecordingDestinationKind::Folder,
            profile_id: None,
            profile_name: None,
            volume: None,
        }
    }

    /// A sink over a real session folder, with the start manifest REMOVED.
    ///
    /// `recording_start` writes `manifest.json` once before the sidecar spawns
    /// (`create_with_meta`) and the sink writes it once at finalize. Deleting the
    /// first one turns "how many times does a session write its metadata" into a
    /// question about a file that either exists or does not — a stronger answer
    /// than a counter the production path would have to carry for the tests.
    fn recording_sink_in(folder: &Path, sync: Option<RecordingSyncSession>) -> RecordingSink {
        recording_sink_indexed(folder, sync, None)
    }

    /// [`recording_sink_in`], plus the Story 42.1 archive half a session carries
    /// when the app has an `archive.db` open.
    ///
    /// The manifest is created WITH metadata here (unlike the 41.5 path it
    /// replaced) because a completion row is supposed to carry the session's
    /// title, participants, note, tags and custom fields — a sink over a
    /// meta-less manifest could only ever prove that nulls arrive.
    fn recording_sink_indexed(
        folder: &Path,
        sync: Option<RecordingSyncSession>,
        archive: Option<Arc<RecordingArchiveSession>>,
    ) -> RecordingSink {
        let manifest = SessionManifest::create_with_meta(
            folder.to_path_buf(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(indexed_meta()),
            Some(INDEXED_STARTED_AT.to_owned()),
        )
        .expect("create session folder + manifest");
        std::fs::remove_file(folder.join("manifest.json")).expect("clear the start manifest");
        RecordingSink {
            machine: RecordingSession::new(),
            manifest,
            status: Arc::new(Mutex::new(RecordingStatusVm::idle())),
            platform: Arc::new(CapturingPlatform::new()),
            sync,
            archive,
        }
    }

    /// Close one segment the way a rotation does: the sidecar renames the file
    /// onto its final name (Story 41.3) and only then reports it.
    fn close_segment(sink: &mut RecordingSink, index: u32) {
        let path = sink
            .manifest
            .folder()
            .join(format!("screen-{index:04}.mov"));
        std::fs::write(&path, vec![7u8; 64]).expect("segment file");
        sink.handle(RecordingEvent::SegmentClosed {
            index,
            path: Some(path.to_string_lossy().into_owned()),
            bytes: Some(64),
            track: Some("screen".to_owned()),
            pts_start: Some(f64::from(index)),
            pts_end: Some(f64::from(index) + 1.0),
        });
    }

    /// A four-hour session, synthetically: preflight, capture, `rotations` closed
    /// segments, stop, finalize. 48 rotations is the AC's session; a real sidecar
    /// would take four hours to say the same thing.
    fn drive_synthetic_session(sink: &mut RecordingSink, rotations: u32) {
        sink.handle(RecordingEvent::PreflightStarted);
        sink.handle(RecordingEvent::CaptureStarted);
        for index in 0..rotations {
            close_segment(sink, index);
        }
        sink.handle(RecordingEvent::Stopping);
        sink.handle(RecordingEvent::Finalized);
    }

    /// The AC's counters in one session (FR-137, FR-146): 48 rotations produce 48
    /// ledger lines and 48 assertions, ONE `.gitattributes` write and ONE
    /// `manifest.json` write — the metadata of a live session is not rewritten
    /// under the recorder that is still filling the folder.
    #[test]
    fn a_forty_eight_rotation_session_writes_one_lfs_rule_one_manifest_and_asserts_every_segment() {
        let root = scan_temp_dir("rec-41-5-counts");
        let folder = root.join("keeper-rec session");
        let port = Arc::new(CountingSyncPort::new(SessionPushPolicy::AtSessionEnd));
        let sync = begin_recording_sync(&profile_destination(&root), "mov", Some(port.clone()))
            .expect("a profile destination opens the sync seam");
        let mut sink = recording_sink_in(&folder, Some(sync));

        // The rule is written at START, before a single event is folded — the
        // working tree does not change under a running recorder.
        assert_eq!(port.count(&SyncCall::EnsureLfsRule("mov".to_owned())), 1);
        assert_eq!(
            port.count(&SyncCall::PushPolicy),
            1,
            "the policy in force is read once, at start"
        );

        sink.handle(RecordingEvent::PreflightStarted);
        sink.handle(RecordingEvent::CaptureStarted);
        for index in 0..48 {
            close_segment(&mut sink, index);
        }

        assert_eq!(
            sink.manifest.segments.len(),
            48,
            "one ledger line per closed segment"
        );
        assert_eq!(
            port.assertions(),
            48,
            "one finished-path assertion per closed segment"
        );
        assert_eq!(
            port.count(&SyncCall::EnsureLfsRule("mov".to_owned())),
            1,
            "48 rotations must not touch `.gitattributes` again"
        );
        assert!(
            !folder.join("manifest.json").exists(),
            "a live session rewrote its metadata mid-recording"
        );

        sink.handle(RecordingEvent::Stopping);
        sink.handle(RecordingEvent::Finalized);

        let written = SessionManifest::load(&folder).expect("the finalized manifest");
        assert!(matches!(written.status, ManifestStatus::Finalized));
        assert_eq!(
            written.segments.len(),
            48,
            "the one write carries every segment"
        );
        assert_eq!(
            written.segments[7].pts_start,
            Some(7.0),
            "the terminal reconcile kept the host-clock bounds by index"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The default policy publishes nothing during the meeting (FR-136, AD-70):
    /// durability is immediate, publication waits for the session to end.
    #[test]
    fn the_default_session_end_policy_asks_for_no_push_until_the_session_ends() {
        let root = scan_temp_dir("rec-41-5-session-end");
        let folder = root.join("keeper-rec session");
        let port = Arc::new(CountingSyncPort::new(SessionPushPolicy::AtSessionEnd));
        let sync = begin_recording_sync(&profile_destination(&root), "mov", Some(port.clone()))
            .expect("the sync seam");
        let mut sink = recording_sink_in(&folder, Some(sync));

        sink.handle(RecordingEvent::PreflightStarted);
        sink.handle(RecordingEvent::CaptureStarted);
        for index in 0..48 {
            close_segment(&mut sink, index);
        }
        assert_eq!(
            port.pushes(RecordingPushTrigger::SegmentCommitted),
            0,
            "no push may be asked for while the recorder is running"
        );
        assert_eq!(port.pushes(RecordingPushTrigger::SessionEnd), 0);

        sink.handle(RecordingEvent::Stopping);
        sink.handle(RecordingEvent::Finalized);

        assert_eq!(
            port.pushes(RecordingPushTrigger::SessionEnd),
            1,
            "exactly one push, and only once the session ended"
        );
        assert_eq!(port.pushes(RecordingPushTrigger::SegmentCommitted), 0);
        assert_eq!(
            port.calls().last(),
            Some(&SyncCall::Push(RecordingPushTrigger::SessionEnd)),
            "the session's last word to the engine is its push"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The two policies that publish mid-session ask on every committed segment.
    ///
    /// `Immediate` means now; `Window` means the engine's clock decides, and the
    /// only way it can decide is to be asked — the window is never evaluated here
    /// (a second implementation of the quiet hours is how two answers happen).
    #[test]
    fn a_policy_that_publishes_mid_session_asks_at_every_closed_segment() {
        for policy in [
            SessionPushPolicy::PerSegment,
            SessionPushPolicy::InQuietHours,
        ] {
            let root = scan_temp_dir("rec-41-5-mid-session");
            let folder = root.join("keeper-rec session");
            let port = Arc::new(CountingSyncPort::new(policy));
            let sync = begin_recording_sync(&profile_destination(&root), "mov", Some(port.clone()))
                .expect("the sync seam");
            let mut sink = recording_sink_in(&folder, Some(sync));
            drive_synthetic_session(&mut sink, 48);

            assert_eq!(
                port.pushes(RecordingPushTrigger::SegmentCommitted),
                48,
                "{policy:?}: one push request per committed segment"
            );
            assert_eq!(
                port.pushes(RecordingPushTrigger::SessionEnd),
                1,
                "{policy:?}: the finalized manifest's own commit is still published once"
            );
            assert_eq!(port.assertions(), 48, "{policy:?}");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// A plain folder is not a synced folder, and the sink must not treat it like
    /// one: no assertion, no push, no engine call at all — while the recording
    /// itself is exactly as complete.
    #[test]
    fn a_plain_folder_destination_makes_no_engine_call_at_all() {
        let root = scan_temp_dir("rec-41-5-folder");
        let folder = root.join("keeper-rec session");
        let port = Arc::new(CountingSyncPort::new(SessionPushPolicy::PerSegment));
        assert!(
            begin_recording_sync(&folder_destination(&root), "mov", Some(port.clone())).is_none(),
            "there is no profile to open a seam onto"
        );

        let mut sink = recording_sink_in(&folder, None);
        drive_synthetic_session(&mut sink, 48);

        assert!(
            port.calls().is_empty(),
            "a plain folder asked the engine for something: {:?}",
            port.calls()
        );
        let written = SessionManifest::load(&folder).expect("the finalized manifest");
        assert_eq!(
            written.segments.len(),
            48,
            "the ledger is the recorder's own"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The epic's posture in one test (NFR-34): an engine that refuses everything
    /// — the LFS rule fails, every assertion is dropped — costs the recording
    /// nothing. Not one ledger line, not the finalize, not the manifest.
    #[test]
    fn an_engine_that_refuses_everything_does_not_cost_a_single_ledger_line() {
        let root = scan_temp_dir("rec-41-5-refusing");
        let folder = root.join("keeper-rec session");
        let port = Arc::new(CountingSyncPort::refusing());
        let sync = begin_recording_sync(&profile_destination(&root), "mov", Some(port.clone()))
            .expect("the seam opens even against an engine that refuses everything");
        let mut sink = recording_sink_in(&folder, Some(sync));
        drive_synthetic_session(&mut sink, 48);

        let written = SessionManifest::load(&folder).expect("the finalized manifest");
        assert_eq!(written.segments.len(), 48);
        assert!(matches!(written.status, ManifestStatus::Finalized));
        assert_eq!(
            status_lock(&sink.status).state,
            RecordingUiState::Finalized,
            "a refused sync must never surface as a failed session"
        );
        assert_eq!(
            port.count(&SyncCall::EnsureLfsRule("mov".to_owned())),
            1,
            "a refused rule is not retried per rotation"
        );
        assert_eq!(port.assertions(), 48, "every segment is still asserted");
        assert_eq!(
            port.pushes(RecordingPushTrigger::SessionEnd),
            1,
            "the session still says it ended; the engine still decides what that costs"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The matrix's "assertion refused" row, answered before the engine has to: a
    /// session folder outside the profile's recordings root asserts nothing, takes
    /// the ordinary settle path, and records exactly as usual.
    #[test]
    fn a_session_outside_the_recordings_root_asserts_nothing_and_still_records() {
        let root = scan_temp_dir("rec-41-5-outside");
        let recordings_root = root.join("recordings");
        std::fs::create_dir_all(&recordings_root).expect("recordings root");
        let folder = root.join("elsewhere");
        let port = Arc::new(CountingSyncPort::new(SessionPushPolicy::PerSegment));
        let sync = begin_recording_sync(
            &profile_destination(&recordings_root),
            "mov",
            Some(port.clone()),
        )
        .expect("the sync seam");
        let mut sink = recording_sink_in(&folder, Some(sync));
        drive_synthetic_session(&mut sink, 3);

        assert_eq!(
            port.assertions(),
            0,
            "a path the engine would refuse is never asserted"
        );
        assert_eq!(
            port.pushes(RecordingPushTrigger::SegmentCommitted),
            0,
            "nothing was committed here, so there is nothing to publish per segment"
        );
        let written = SessionManifest::load(&folder).expect("the finalized manifest");
        assert_eq!(written.segments.len(), 3, "the recording is unaffected");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// FR-137's rule covers "the session's media extension", so the seed name and
    /// the extension named beside it have to be the same fact.
    #[test]
    fn the_session_seed_name_carries_the_extension_its_lfs_rule_names() {
        for audio_only in [false, true] {
            let (name, extension) = session_media_seed(audio_only);
            assert_eq!(
                Path::new(name).extension().and_then(|ext| ext.to_str()),
                Some(extension),
                "{name} does not carry .{extension}"
            );
        }
    }

    // --- Story 41.6: durability you can read --------------------------------

    /// One engine answer, spelled as the facts the engine actually holds.
    fn facts(committed: bool, pushed: bool, verified: bool) -> Result<SegmentDurability, String> {
        Ok(SegmentDurability {
            committed,
            pushed,
            verified,
            problem: None,
        })
    }

    /// The engine's reading of a session whose commits exist and whose push the
    /// remote refused — the protected-branch and killed-network rows.
    fn refused(reason: &str) -> Result<SegmentDurability, String> {
        Ok(SegmentDurability {
            committed: true,
            pushed: false,
            verified: false,
            problem: Some(reason.to_owned()),
        })
    }

    /// A run slot exactly as `recording_start` leaves one for a PROFILE
    /// destination: a live snapshot naming the session folder, plus the
    /// durability reader over the scripted port.
    fn durability_slot(folder: &Path, port: Arc<CountingSyncPort>) -> Mutex<Option<RecordingRun>> {
        durability_slot_indexed(folder, port, None)
    }

    /// [`durability_slot`], plus the Story 42.1 archive half the reader updates
    /// when the floor climbs.
    fn durability_slot_indexed(
        folder: &Path,
        port: Arc<CountingSyncPort>,
        archive: Option<Arc<RecordingArchiveSession>>,
    ) -> Mutex<Option<RecordingRun>> {
        let mut snapshot = RecordingStatusVm::idle();
        snapshot.state = RecordingUiState::Recording;
        snapshot.output_path = Some(folder.to_string_lossy().into_owned());
        Mutex::new(Some(RecordingRun {
            stop_tx: None,
            status: Arc::new(Mutex::new(snapshot)),
            driver: None,
            segment_cap_mb: 500,
            destination_dir: folder.to_path_buf(),
            durability: Some(Arc::new(RecordingDurabilityReader::new(
                "profile-1".to_owned(),
                port,
                archive,
            ))),
        }))
    }

    /// One turn of the ~1 Hz status poll, through the real two halves the
    /// `recording_status` command runs — never the reader in isolation, so what
    /// these tests assert is what the surface receives.
    fn poll_durability(slot: &Mutex<Option<RecordingRun>>) -> RecordingDurabilityVm {
        let (snapshot, cap, reader) = live_snapshot(slot).expect("a live slot");
        with_disk_figures(snapshot, cap, reader.as_deref()).durability
    }

    /// The ranking, in one place: the strongest true fact wins. The last row is
    /// the one that matters — a partially-updated set (pushed recorded before
    /// committed) is optimistic by one rung rather than nonsense.
    #[test]
    fn durability_state_reads_the_strongest_true_fact() {
        use RecordingDurabilityState::*;
        let read = |c, p, v| {
            durability_state(&SegmentDurability {
                committed: c,
                pushed: p,
                verified: v,
                problem: None,
            })
        };
        assert_eq!(read(false, false, false), Local);
        assert_eq!(read(true, false, false), Committed);
        assert_eq!(read(true, true, false), Pushed);
        assert_eq!(read(true, true, true), Verified);
        assert_eq!(read(false, true, false), Pushed);
        assert_eq!(read(false, false, true), Verified);
        // The ordering the floor is a `max` over.
        assert!(Local < Committed && Committed < Pushed && Pushed < Verified);
    }

    /// The matrix's four advancing rows, in the order a real rotation produces
    /// them: nothing committed yet ⇒ `local`; the segment is in a commit ⇒
    /// `committed`; the commit is on the remote ⇒ `pushed`; the engine verified
    /// the objects ⇒ `verified`. One ask per poll, and no problem ever named.
    #[test]
    fn a_profile_session_advances_local_committed_pushed_verified() {
        let folder = scan_temp_dir("rec-41-6-advance");
        let port = Arc::new(CountingSyncPort::scripted(vec![
            facts(false, false, false),
            facts(true, false, false),
            facts(true, true, false),
            facts(true, true, true),
        ]));
        let slot = durability_slot(&folder, Arc::clone(&port));

        for expected in [
            RecordingDurabilityState::Local,
            RecordingDurabilityState::Committed,
            RecordingDurabilityState::Pushed,
            RecordingDurabilityState::Verified,
        ] {
            let reading = poll_durability(&slot);
            assert_eq!(reading.state, expected);
            assert_eq!(reading.detail, None, "{expected:?} named a problem");
        }
        assert_eq!(port.durability_asks(), 4, "one ask per poll, no more");
        assert_eq!(
            port.calls()
                .iter()
                .filter(|call| !matches!(call, SyncCall::Durability(_)))
                .count(),
            0,
            "reading durability must not commit, push or assert anything"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The question is asked about the SESSION FOLDER — taken from the snapshot,
    /// so a retitle (Story 40.4) moves it with the session rather than leaving
    /// the reader asking about a folder that no longer exists.
    #[cfg(desktop)]
    #[test]
    fn the_durability_question_names_the_sessions_current_folder() {
        let folder = scan_temp_dir("rec-41-6-folder");
        let moved = folder.with_file_name("rec-41-6-folder standup");
        let port = Arc::new(CountingSyncPort::scripted(vec![facts(true, false, false)]));
        let slot = durability_slot(&folder, Arc::clone(&port));
        poll_durability(&slot);

        assert!(repoint_recording_slot_output(&slot, &folder, &moved));
        poll_durability(&slot);

        assert_eq!(
            port.calls(),
            vec![
                SyncCall::Durability(folder.clone()),
                SyncCall::Durability(moved),
            ]
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The matrix's plain-folder row, and its "engine unavailable (no git)" row
    /// with it: both leave the run with no reader, both read `local`, and
    /// neither asks the engine a single question — there is no profile, so there
    /// is no further promise to make.
    #[test]
    fn a_plain_folder_session_reads_local_and_never_asks_the_engine() {
        let folder = scan_temp_dir("rec-41-6-plain");
        let port = Arc::new(CountingSyncPort::scripted(vec![facts(true, true, true)]));
        // What `recording_start` builds for a folder destination: no sync
        // session, therefore no reader.
        assert!(
            begin_recording_sync(&folder_destination(&folder), "mov", Some(port.clone())).is_none()
        );
        let mut snapshot = RecordingStatusVm::idle();
        snapshot.state = RecordingUiState::Recording;
        snapshot.output_path = Some(folder.to_string_lossy().into_owned());
        let slot = Mutex::new(Some(RecordingRun {
            stop_tx: None,
            status: Arc::new(Mutex::new(snapshot)),
            driver: None,
            segment_cap_mb: 500,
            destination_dir: folder.clone(),
            durability: None,
        }));

        for _ in 0..3 {
            assert_eq!(
                poll_durability(&slot),
                RecordingDurabilityVm::local(),
                "a plain folder is `local` and says so plainly"
            );
        }
        assert_eq!(
            port.durability_asks(),
            0,
            "a plain folder asked the engine about durability"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The matrix's mid-session-mix row, which is the whole reason the state is a
    /// floor: segment 3 is pushed while segment 4 is still settling, and the
    /// engine's answer for the folder drops back. The line must NOT walk
    /// backwards — "would what I have recorded survive?" is a question about the
    /// worst case of what is already captured, and that only ever improves.
    #[test]
    fn the_durability_floor_never_regresses_within_a_session() {
        let folder = scan_temp_dir("rec-41-6-floor");
        let port = Arc::new(CountingSyncPort::scripted(vec![
            facts(true, true, false),
            facts(true, false, false),
            facts(false, false, false),
            facts(true, false, false),
        ]));
        let slot = durability_slot(&folder, Arc::clone(&port));

        assert_eq!(
            poll_durability(&slot).state,
            RecordingDurabilityState::Pushed
        );
        for _ in 0..3 {
            assert_eq!(
                poll_durability(&slot).state,
                RecordingDurabilityState::Pushed,
                "a later segment still settling walked the session backwards"
            );
        }

        // And it still CLIMBS: a floor is a `max`, not a latch on the first
        // answer.
        let climbing = Arc::new(CountingSyncPort::scripted(vec![facts(true, true, true)]));
        let slot = durability_slot(&folder, climbing);
        assert_eq!(
            poll_durability(&slot).state,
            RecordingDurabilityState::Verified
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The matrix's protected-branch and killed-network rows, which are the same
    /// reading: the commits exist, the publication did not happen, and the
    /// remote's own sentence is carried verbatim so no surface has to invent sync
    /// language. The state stays `committed` — a refused push is not a failed
    /// recording.
    #[test]
    fn a_refused_push_stays_committed_and_carries_the_reason_verbatim() {
        let folder = scan_temp_dir("rec-41-6-refused");
        let reason = "push rejected: protected branch main";
        let port = Arc::new(CountingSyncPort::scripted(vec![
            facts(true, false, false),
            refused(reason),
        ]));
        let slot = durability_slot(&folder, port);

        assert_eq!(poll_durability(&slot).detail, None);
        let reading = poll_durability(&slot);
        assert_eq!(reading.state, RecordingDurabilityState::Committed);
        assert_eq!(
            reading.detail.as_deref(),
            Some(reason),
            "the reason must reach the surface unedited"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The reason does NOT floor. It names what is wrong NOW, so a push that
    /// succeeds later clears it — a latched reason would outlive the problem it
    /// describes and leave the banner warning about a resolved outage forever.
    #[test]
    fn a_reason_clears_once_publication_succeeds_while_the_state_holds() {
        let folder = scan_temp_dir("rec-41-6-reason-clears");
        let port = Arc::new(CountingSyncPort::scripted(vec![
            refused("push rejected: non-fast-forward"),
            facts(true, true, false),
        ]));
        let slot = durability_slot(&folder, port);

        assert!(poll_durability(&slot).detail.is_some());
        let reading = poll_durability(&slot);
        assert_eq!(reading.state, RecordingDurabilityState::Pushed);
        assert_eq!(reading.detail, None, "a resolved problem kept warning");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The matrix's "engine query fails" row (NFR-34): a transient read failure
    /// keeps the LAST KNOWN state — never `local` after `pushed`, never an error
    /// on a poll the banner depends on — and spends exactly ONE log line on the
    /// outage. The `degraded` latch is what makes that one line one line: it is
    /// set by the first failure and cleared by the next success, so an hour of
    /// failures is one `warn` and a SECOND outage is still heard.
    #[test]
    fn a_failed_engine_read_keeps_the_last_known_state_and_logs_once() {
        let folder = scan_temp_dir("rec-41-6-degrade");
        let port = Arc::new(CountingSyncPort::scripted(vec![
            facts(true, true, false),
            Err("the index is locked".to_owned()),
            Err("the index is locked".to_owned()),
            Err("the index is locked".to_owned()),
            facts(true, true, false),
        ]));
        let reader = RecordingDurabilityReader::new("profile-1".to_owned(), port, None);

        assert_eq!(reader.read(&folder).state, RecordingDurabilityState::Pushed);
        assert!(!reader.degraded.load(Ordering::Relaxed));

        for turn in 0..3 {
            let reading = reader.read(&folder);
            assert_eq!(
                reading.state,
                RecordingDurabilityState::Pushed,
                "failure {turn} lost the last known state"
            );
            assert!(
                reader.degraded.load(Ordering::Relaxed),
                "the outage must latch after the first line"
            );
        }

        assert_eq!(reader.read(&folder).state, RecordingDurabilityState::Pushed);
        assert!(
            !reader.degraded.load(Ordering::Relaxed),
            "a recovered engine must be able to report a NEW outage"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The matrix's no-session rows: with nothing recording — an iOS build, a
    /// build without the recording capability, or simply before the first start
    /// — the snapshot is the honest idle default, and its durability is `local`
    /// with nothing to explain.
    #[test]
    fn the_idle_snapshot_reads_local_with_no_reason() {
        assert_eq!(
            RecordingStatusVm::idle().durability,
            RecordingDurabilityVm::local()
        );
        let empty: Mutex<Option<RecordingRun>> = Mutex::new(None);
        assert!(live_snapshot(&empty).is_none());
    }

    // --- Story 42.1: a session is a row -------------------------------------

    /// A session id exactly as Story 40.3 mints one: `<device>-<session>`, both
    /// halves Crockford and therefore `-`-free.
    const INDEXED_SESSION_ID: &str = "01JQDEVICE0000000000000000-01JQSESSION000000000000000";

    /// The device half of [`INDEXED_SESSION_ID`], spelled out rather than derived,
    /// so a broken split is a failing test and not a matching bug.
    const INDEXED_DEVICE_ID: &str = "01JQDEVICE0000000000000000";

    /// The wall-clock start every indexed test session carries.
    ///
    /// Deliberately a date in the PAST, and never today's: `finalize` stamps
    /// `endedAt` from the real clock, so a fixture whose start is in the future
    /// produces a completed row that ended before it began. Today's date did
    /// exactly that — the suite runs on a macOS host west of UTC, where
    /// 10:00+02:00 on the current day is still hours away — and the assertion
    /// below would have started passing on its own a few hours later, which is
    /// the worst way for a test to be wrong.
    const INDEXED_STARTED_AT: &str = "2026-01-02T10:00:00+02:00";

    /// The metadata an indexed test session carries, covering every column the
    /// completion row is supposed to fill: two plain-text fields, a free-text
    /// one, a tag list and a custom pair.
    fn indexed_meta() -> keeper_core::recording::SessionMeta {
        keeper_core::recording::SessionMeta {
            session_id: Some(INDEXED_SESSION_ID.to_owned()),
            title: Some("Weekly sync".to_owned()),
            participants: Some("Ada, Grace".to_owned()),
            note: Some("recorded for the archive".to_owned()),
            tags: Some(vec!["standup".to_owned(), "eng".to_owned()]),
            custom: Some(vec![keeper_core::recording::SessionMetaField {
                name: "room".to_owned(),
                value: "Blue".to_owned(),
            }]),
        }
    }

    /// One write a session made into the archive, in the order it was made.
    ///
    /// The row types carry `PartialEq`, so whole-row equality is available — but
    /// the assertions that matter here are counts and individual columns (is the
    /// path relative? does the root kind name the folder case?), which is what the
    /// accessors below are shaped for.
    #[derive(Debug, Clone, PartialEq)]
    enum ArchiveWrite {
        Started(RecordingRow),
        Segment(RecordingSegmentRow),
        Finalized(RecordingRow),
        Durability(String, RecordingDurabilityState),
        Moved(String, String),
    }

    /// A [`RecordingArchivePort`] that writes into a `Vec` (Story 42.1).
    ///
    /// The story's acceptance is stated in counts — one insert per start, one row
    /// per closed segment, one completion per finalize, one update per durability
    /// MOVE — and a count needs something to count. A real
    /// [`keeper_core::archive::ArchiveHandle`] would answer the same questions
    /// with a channel, a writer task and a SQLite file; this answers them on the
    /// same seam the production path calls.
    #[derive(Default)]
    struct ArchiveSpy {
        writes: Mutex<Vec<ArchiveWrite>>,
    }

    impl ArchiveSpy {
        fn writes(&self) -> Vec<ArchiveWrite> {
            self.writes.lock().expect("lock archive writes").clone()
        }

        fn push(&self, write: ArchiveWrite) {
            self.writes.lock().expect("lock archive writes").push(write);
        }

        /// The start inserts, in order.
        fn starts(&self) -> Vec<RecordingRow> {
            self.writes()
                .into_iter()
                .filter_map(|write| match write {
                    ArchiveWrite::Started(row) => Some(row),
                    _ => None,
                })
                .collect()
        }

        /// The completion upserts, in order.
        fn completions(&self) -> Vec<RecordingRow> {
            self.writes()
                .into_iter()
                .filter_map(|write| match write {
                    ArchiveWrite::Finalized(row) => Some(row),
                    _ => None,
                })
                .collect()
        }

        /// The segment rows, in the order the rotations closed.
        fn segments(&self) -> Vec<RecordingSegmentRow> {
            self.writes()
                .into_iter()
                .filter_map(|write| match write {
                    ArchiveWrite::Segment(row) => Some(row),
                    _ => None,
                })
                .collect()
        }

        /// The durability updates, in the order the floor climbed.
        fn durability_updates(&self) -> Vec<(String, RecordingDurabilityState)> {
            self.writes()
                .into_iter()
                .filter_map(|write| match write {
                    ArchiveWrite::Durability(session_id, state) => Some((session_id, state)),
                    _ => None,
                })
                .collect()
        }

        /// The retitle repoints, in order: `(session_id, relative_path)`.
        fn moves(&self) -> Vec<(String, String)> {
            self.writes()
                .into_iter()
                .filter_map(|write| match write {
                    ArchiveWrite::Moved(session_id, relative_path) => {
                        Some((session_id, relative_path))
                    }
                    _ => None,
                })
                .collect()
        }
    }

    impl RecordingArchivePort for ArchiveSpy {
        fn record_started(&self, row: RecordingRow) {
            self.push(ArchiveWrite::Started(row));
        }

        fn record_segment(&self, row: RecordingSegmentRow) {
            self.push(ArchiveWrite::Segment(row));
        }

        fn record_finalized(&self, row: RecordingRow) {
            self.push(ArchiveWrite::Finalized(row));
        }

        fn record_durability(&self, session_id: &str, state: RecordingDurabilityState) {
            self.push(ArchiveWrite::Durability(session_id.to_owned(), state));
        }

        fn record_moved(&self, session_id: &str, relative_path: &str) {
            self.push(ArchiveWrite::Moved(
                session_id.to_owned(),
                relative_path.to_owned(),
            ));
        }
    }

    /// An archive whose writer channel is closed: every send is accepted and goes
    /// nowhere, which is precisely what
    /// [`keeper_core::archive::ArchiveHandle`] does once its task has ended.
    ///
    /// It counts the attempts rather than the writes, because the port contract is
    /// infallible by design — the only failure a caller can express is that the
    /// write went nowhere, and the only thing worth proving is that every send
    /// SITE still ran and the recorder could not tell.
    #[derive(Default)]
    struct DroppingArchive {
        attempts: Mutex<usize>,
    }

    impl DroppingArchive {
        fn attempts(&self) -> usize {
            *self.attempts.lock().expect("lock dropped attempts")
        }

        fn drop_one(&self) {
            *self.attempts.lock().expect("lock dropped attempts") += 1;
        }
    }

    impl RecordingArchivePort for DroppingArchive {
        fn record_started(&self, _row: RecordingRow) {
            self.drop_one();
        }

        fn record_segment(&self, _row: RecordingSegmentRow) {
            self.drop_one();
        }

        fn record_finalized(&self, _row: RecordingRow) {
            self.drop_one();
        }

        fn record_durability(&self, _session_id: &str, _state: RecordingDurabilityState) {
            self.drop_one();
        }

        fn record_moved(&self, _session_id: &str, _relative_path: &str) {
            self.drop_one();
        }
    }

    /// The archive half `recording_start` builds, over an arbitrary port.
    fn archive_session(
        port: Arc<dyn RecordingArchivePort>,
        destination: &RecordingDestination,
    ) -> Arc<RecordingArchiveSession> {
        Arc::new(RecordingArchiveSession::open(
            port,
            INDEXED_SESSION_ID.to_owned(),
            destination,
            "hevc".to_owned(),
            30,
        ))
    }

    /// The matrix's session-start row: one insert, and every path in it RELATIVE
    /// to the destination root — which is what lets a retitle move the folder and
    /// a clone carry the tree onto another machine without invalidating the row.
    #[test]
    fn a_session_start_indexes_exactly_one_row_with_a_relative_path() {
        let root = scan_temp_dir("rec-42-1-start");
        // The template nests since Story 40.3, so the relative path is a path and
        // not a basename — the case a `file_name()` shortcut would get wrong.
        let folder = root.join("2026").join("2026-01-02 1000");
        std::fs::create_dir_all(root.join("2026")).expect("the template's year level");
        let spy = Arc::new(ArchiveSpy::default());
        let archive = archive_session(spy.clone(), &profile_destination(&root));
        let sink = recording_sink_indexed(&folder, None, Some(archive.clone()));

        archive.started(&sink.manifest);

        let rows = spy.starts();
        assert_eq!(rows.len(), 1, "a start is one insert");
        assert_eq!(spy.writes().len(), 1, "a start writes nothing else");
        let row = &rows[0];
        assert_eq!(row.session_id, INDEXED_SESSION_ID);
        assert_eq!(row.device_id.as_deref(), Some(INDEXED_DEVICE_ID));
        assert_eq!(row.relative_path, "2026/2026-01-02 1000");
        assert!(
            !Path::new(&row.relative_path).is_absolute(),
            "an absolute path reached a column"
        );
        assert!(
            !row.relative_path.contains(&*root.to_string_lossy()),
            "the row carries the machine's own root: {}",
            row.relative_path
        );
        assert_eq!(row.root_kind, "profile");
        assert_eq!(row.profile_id.as_deref(), Some("profile-1"));
        assert_eq!(
            row.started_ts,
            Some(
                DateTime::parse_from_rfc3339(INDEXED_STARTED_AT)
                    .expect("the fixture stamp parses")
                    .timestamp_millis()
            ),
            "the row's start is the manifest's own stamp, in epoch ms"
        );
        assert_eq!(
            row.ended_ts, None,
            "a session that just began has not ended"
        );
        assert_eq!(row.title.as_deref(), Some("Weekly sync"));
        assert_eq!(
            row.participants_json.as_deref(),
            Some(r#""Ada, Grace""#),
            "participants is free text, stored as the JSON string it is"
        );
        assert_eq!(row.note.as_deref(), Some("recorded for the archive"));
        assert_eq!(row.tags_json.as_deref(), Some(r#"["standup","eng"]"#));
        assert_eq!(
            row.custom_json.as_deref(),
            Some(r#"[{"name":"room","value":"Blue"}]"#)
        );
        assert_eq!(row.codec.as_deref(), Some("hevc"));
        assert_eq!(row.fps, Some(30));
        assert_eq!(
            row.durability, "local",
            "a session that has just begun is on this Mac and nowhere else"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The AC's four-hour session, counted: 48 rotations are 48 segment rows and
    /// not one extra insert. The sink indexes segments and the completion; the
    /// session's insert belongs to the start path and the sink must not repeat it.
    #[test]
    fn forty_eight_rotations_index_forty_eight_segment_rows_and_no_extra_insert() {
        let root = scan_temp_dir("rec-42-1-rotations");
        let folder = root.join("keeper-rec session");
        let spy = Arc::new(ArchiveSpy::default());
        let archive = archive_session(spy.clone(), &profile_destination(&root));
        let port = Arc::new(CountingSyncPort::new(SessionPushPolicy::AtSessionEnd));
        let sync = begin_recording_sync(&profile_destination(&root), "mov", Some(port.clone()))
            .expect("the sync seam");
        let mut sink = recording_sink_indexed(&folder, Some(sync), Some(archive));

        sink.handle(RecordingEvent::PreflightStarted);
        sink.handle(RecordingEvent::CaptureStarted);
        for index in 0..48 {
            close_segment(&mut sink, index);
        }

        let segments = spy.segments();
        assert_eq!(segments.len(), 48, "one row per closed segment");
        assert_eq!(
            spy.starts().len(),
            0,
            "the sink inserted a session row; that is the start path's write"
        );
        assert_eq!(
            spy.completions().len(),
            0,
            "a live session was completed mid-recording"
        );
        assert_eq!(
            segments[7].relative_path, "keeper-rec session/screen-0007.mov",
            "a segment row must name the file relative to the destination root"
        );
        assert_eq!(segments[7].index, 7);
        assert_eq!(segments[7].track, "screen");
        assert_eq!(segments[7].bytes, 64);
        assert_eq!(segments[7].pts_start, Some(7.0));
        assert_eq!(segments[7].pts_end, Some(8.0));
        assert!(
            segments[7].closed_ts.is_some_and(|ts| ts > 0),
            "a closed segment is stamped with when this host saw it close"
        );
        assert_eq!(segments[7].session_id, INDEXED_SESSION_ID);
        assert!(
            segments
                .iter()
                .all(|row| !Path::new(&row.relative_path).is_absolute()),
            "an absolute path reached a segment row"
        );

        sink.handle(RecordingEvent::Stopping);
        sink.handle(RecordingEvent::Finalized);
        assert_eq!(spy.segments().len(), 48, "finalizing invented a segment");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The matrix's duplicate-finalize row, answered honestly at THIS seam.
    ///
    /// The sink sends exactly ONE completion per session, and it cannot be made to
    /// send two through its own front door: `handle` only finalizes on a terminal
    /// transition, and the machine rejects a second terminal event, so the second
    /// `Finalized` never reaches `finalize` at all. Called directly — which the
    /// production path never does — `finalize` sends a second completion, and that
    /// is deliberate: it is the same `INSERT OR REPLACE` on the same session id, so
    /// the archive still holds one row. The safety net exists; the code does not
    /// lean on it.
    #[test]
    fn finalize_sends_one_completion_and_a_repeat_is_the_same_upsert() {
        let root = scan_temp_dir("rec-42-1-finalize");
        let folder = root.join("keeper-rec session");
        let spy = Arc::new(ArchiveSpy::default());
        let archive = archive_session(spy.clone(), &folder_destination(&root));
        let mut sink = recording_sink_indexed(&folder, None, Some(archive));

        drive_synthetic_session(&mut sink, 3);

        let completions = spy.completions();
        assert_eq!(completions.len(), 1, "a session completes its row once");
        let row = &completions[0];
        assert_eq!(row.session_id, INDEXED_SESSION_ID);
        assert_eq!(row.relative_path, "keeper-rec session");
        assert_eq!(row.title.as_deref(), Some("Weekly sync"));
        assert_eq!(row.participants_json.as_deref(), Some(r#""Ada, Grace""#));
        assert_eq!(row.note.as_deref(), Some("recorded for the archive"));
        assert_eq!(row.tags_json.as_deref(), Some(r#"["standup","eng"]"#));
        assert_eq!(
            row.custom_json.as_deref(),
            Some(r#"[{"name":"room","value":"Blue"}]"#)
        );
        assert_eq!(row.codec.as_deref(), Some("hevc"));
        assert_eq!(row.fps, Some(30));
        assert_eq!(row.width, None, "nothing in this app knows the frame size");
        assert_eq!(row.height, None);
        assert_eq!(
            row.manifest_version,
            keeper_core::recording::MANIFEST_VERSION
        );
        // Unwrapped rather than compared as `Option`s: `Some(_) >= None` is
        // true, so an `Option` comparison here would still pass if the start
        // stamp stopped being parsed at all.
        let started = row
            .started_ts
            .expect("a completed row must carry when the session began");
        let ended = row
            .ended_ts
            .expect("a completed row must carry when the session ended");
        assert!(
            ended >= started,
            "the session ended before it began: {ended} < {started}"
        );

        // A second terminal event: the machine rejects it, so nothing downstream
        // of the transition happens — no manifest write, no completion.
        sink.handle(RecordingEvent::Finalized);
        assert_eq!(
            spy.completions().len(),
            1,
            "a second terminal event reached the archive through the sink"
        );

        // The direct call the production path never makes: a second, identical
        // upsert onto the same key.
        sink.finalize();
        let completions = spy.completions();
        assert_eq!(completions.len(), 2);
        assert_eq!(
            completions[1].session_id, completions[0].session_id,
            "a duplicate finalize must key onto the same row, never a second one"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- the recording note stub (Story 42.4) --------------------------------

    /// A device half plus a session half, distinct per session so the two-in-a-
    /// minute test is not secretly testing one session twice.
    const STUB_SESSION_A: &str = "01JQDEVICE0000000000000000-01JQSTUBAAAA00000000000000";
    const STUB_SESSION_B: &str = "01JQDEVICE0000000000000000-01JQSTUBBBBB00000000000000";

    /// A sink over a session with a CHOSEN identity and title.
    ///
    /// [`recording_sink_indexed`] fixes both, which is right for the archive
    /// tests and wrong here: a stub is found by its `session:` field, so two
    /// sessions sharing one id would make the second look like a re-finalize of
    /// the first and hide exactly the collision AC5 is about.
    fn note_stub_sink(folder: &Path, session_id: &str, title: Option<&str>) -> RecordingSink {
        let manifest = SessionManifest::create_with_meta(
            folder.to_path_buf(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(keeper_core::recording::SessionMeta {
                session_id: Some(session_id.to_owned()),
                title: title.map(str::to_owned),
                participants: Some("Ada, Grace".to_owned()),
                note: None,
                tags: Some(vec!["standup".to_owned(), "eng".to_owned()]),
                custom: None,
            }),
            Some(INDEXED_STARTED_AT.to_owned()),
        )
        .expect("create session folder + manifest");
        RecordingSink {
            machine: RecordingSession::new(),
            manifest,
            status: Arc::new(Mutex::new(RecordingStatusVm::idle())),
            platform: Arc::new(CapturingPlatform::new()),
            sync: None,
            archive: None,
        }
    }

    /// The `.md` names in a directory, sorted — what a human would see.
    fn markdown_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read the stub directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".md"))
            .collect();
        names.sort();
        names
    }

    /// A vault at `<local_path>/notes`, as the registry would hold one. Roots are
    /// canonical there, so they are canonical here — on macOS `/var` is a symlink
    /// to `/private/var`, and a test that skipped this would compare a path
    /// against a prefix it does not literally start with.
    fn stub_vault(local_path: &Path) -> crate::notes_vault::Vault {
        let root = local_path.join("notes");
        std::fs::create_dir_all(&root).expect("vault root");
        crate::notes_vault::Vault {
            id: "profile-under-test".to_owned(),
            name: "tgdrive".to_owned(),
            root,
            local_path: local_path.to_path_buf(),
            config: keeper_sync::profile::NotesConfig::default(),
            excludes: Arc::new(
                keeper_sync::exclude::ExcludeSet::new(&[]).expect("built-in excludes"),
            ),
        }
    }

    /// A canonical scratch root, so every `strip_prefix` in these tests compares
    /// paths that were spelled the same way.
    fn stub_temp_root(tag: &str) -> PathBuf {
        scan_temp_dir(tag)
            .canonicalize()
            .expect("canonical scratch root")
    }

    /// AC1 at the seam that actually writes the file: stopping a recording leaves
    /// exactly ONE stub, and its frontmatter round-trips through the notes parser
    /// — every key read back unchanged, and the body offset exact to the byte.
    /// Not "it parses".
    #[test]
    fn stopping_a_recording_leaves_one_stub_whose_frontmatter_round_trips() {
        let root = stub_temp_root("rec-42-4-roundtrip");
        let folder = root.join("keeper-rec session");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, Some("Weekly sync"));

        drive_synthetic_session(&mut sink, 2);

        assert_eq!(
            markdown_names(&root),
            vec!["2026-01-02-weekly-sync.md".to_owned()],
            "one stub, named from the session's own title and start date"
        );
        let source = std::fs::read_to_string(root.join("2026-01-02-weekly-sync.md"))
            .expect("the stub is on disk");
        let (fm, body) = Frontmatter::parse(&source);

        assert_eq!(fm.as_string("title"), Some("Weekly sync"));
        assert_eq!(fm.as_string("date"), Some("2026-01-02"));
        assert_eq!(fm.as_string("start"), Some("10:00"));
        assert_eq!(fm.as_string("participants"), Some("Ada, Grace"));
        assert_eq!(
            fm.as_list("tags"),
            Some(vec![
                "standup".to_owned(),
                "eng".to_owned(),
                "recordings".to_owned()
            ]),
            "the session's own tags are carried as stored and keep their order — 42.5 owns \
             resolving them — and story 43.2 appends the one keeper owns, last, because the \
             head of a truncated property row should be the tag the writer chose"
        );
        assert_eq!(
            fm.as_string("session"),
            Some(STUB_SESSION_A),
            "the link carries the immutable identity a retitle leaves alone"
        );
        assert_eq!(
            fm.as_string("recording"),
            Some("keeper-rec session"),
            "and the recording is named relative to the directory they share"
        );
        // The session ended when the sink said so, so these are asserted for
        // presence rather than value — the composer's own tests fix the format.
        assert!(fm.as_string("end").is_some(), "the end time is recorded");
        assert!(fm.as_string("duration").is_some());
        // Story 44.2: the body opens as the recording. Both closed segments are
        // embedded, in the ledger's order, BELOW the heading — `manifest.json`
        // is in `files:` and is not embedded. Asserted here as well as in the
        // composer's own tests because this is the seam that decides what the
        // paths actually look like: they are whatever `stub_files` made
        // relative to the anchor, and nothing joined a root back onto them.
        assert_eq!(
            &source[body..],
            concat!(
                "\n# Weekly sync\n",
                "\n",
                "![[keeper-rec session/screen-0000.mov]]\n",
                "![[keeper-rec session/screen-0001.mov]]\n",
                "\n",
            ),
            "the WHOLE body, so nothing can be reordered without failing here: the body \
             offset is exact, the prose is byte-identical, and the heading is still the \
             first line. An embed above it would become the note's displayed title, \
             because `note_title` falls back to the body's first line"
        );

        // AC4, at the seam where an absolute path could actually get in: the
        // shell knows one and the note must not.
        assert!(
            !source.contains(&root.to_string_lossy().into_owned()),
            "the destination root reached the stub"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// AC5 through the real mechanism: the second stub's name comes from READING
    /// the directory the first one is already in. Both sessions carry the same
    /// title and the same minute-resolution start stamp, which is exactly the
    /// input that would collide if a stamp were treated as a name.
    #[test]
    fn two_sessions_in_the_same_minute_get_two_stubs_and_neither_is_overwritten() {
        let root = stub_temp_root("rec-42-4-collide");
        let mut first = note_stub_sink(
            &root.join("keeper-rec session a"),
            STUB_SESSION_A,
            Some("Weekly sync"),
        );
        let mut second = note_stub_sink(
            &root.join("keeper-rec session b"),
            STUB_SESSION_B,
            Some("Weekly sync"),
        );

        drive_synthetic_session(&mut first, 1);
        drive_synthetic_session(&mut second, 1);

        assert_eq!(
            markdown_names(&root),
            vec![
                "2026-01-02-weekly-sync-2.md".to_owned(),
                "2026-01-02-weekly-sync.md".to_owned(),
            ],
            "the second stub took a free name instead of the first one's"
        );
        // And they are two different sessions' notes, not one written twice.
        let one = std::fs::read_to_string(root.join("2026-01-02-weekly-sync.md")).expect("first");
        let two =
            std::fs::read_to_string(root.join("2026-01-02-weekly-sync-2.md")).expect("second");
        assert_eq!(
            Frontmatter::parse(&one).0.as_string("session"),
            Some(STUB_SESSION_A)
        );
        assert_eq!(
            Frontmatter::parse(&two).0.as_string("session"),
            Some(STUB_SESSION_B)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The matrix's re-finalize row. What must survive is not the file but what
    /// the user put in it, so the stub is edited first: a second write would be
    /// invisible if the bytes were still keeper's own.
    #[test]
    fn a_re_finalize_leaves_an_existing_stub_alone_and_does_not_write_a_second() {
        let root = stub_temp_root("rec-42-4-refinalize");
        let folder = root.join("keeper-rec session");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, Some("Weekly sync"));

        drive_synthetic_session(&mut sink, 1);
        let path = root.join("2026-01-02-weekly-sync.md");
        let composed = std::fs::read_to_string(&path).expect("the first stub");
        let edited = format!("{composed}They agreed to ship on Friday.\n");
        std::fs::write(&path, &edited).expect("the user types");

        // The direct call the production path never makes (the machine rejects a
        // second terminal event) — so the guard is proved rather than assumed.
        sink.finalize();

        assert_eq!(
            markdown_names(&root),
            vec!["2026-01-02-weekly-sync.md".to_owned()],
            "a re-finalize wrote a second stub"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("still there"),
            edited,
            "a re-finalize overwrote what the user had written"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The matrix's write-failure row: logged, and never a recording failure.
    /// The destination is a regular file, so `create_dir_all` refuses it on every
    /// platform — a read-only volume would need root, and a permission bit does
    /// not stop one.
    #[test]
    fn a_stub_that_cannot_be_written_is_swallowed_and_the_session_stays_finalized() {
        let root = stub_temp_root("rec-42-4-writefail");
        let folder = root.join("keeper-rec session");
        let blocked = root.join("blocked");
        std::fs::write(&blocked, b"not a directory").expect("the obstruction");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, Some("Weekly sync"));
        sink.manifest
            .set_ended_at("2026-01-02T11:00:00+02:00".to_owned());
        sink.manifest.write().expect("terminal manifest");

        let dest = StubDestination {
            dir: blocked.clone(),
            anchor: root.clone(),
            vault: None,
        };
        // The whole assertion of "never surfaced": this returns nothing, so there
        // is no failure for the recording path to react to.
        write_recording_note_stub(&sink.manifest, &dest);

        assert!(
            markdown_names(&root).is_empty(),
            "a stub appeared despite the write failing"
        );
        assert_eq!(
            std::fs::read_to_string(&blocked).expect("still a file"),
            "not a directory",
            "the obstruction was clobbered"
        );
        assert!(
            SessionManifest::load(&folder).is_ok(),
            "the session is finalized and its manifest is untouched"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The vault branch. `RecordingsConfig::validate` refuses a recordings root
    /// that overlaps the vault, so the stub lands in a SIBLING subtree — and this
    /// asserts the negative too, because "in the vault" and "not in the
    /// recordings folder" are two claims and only one of them is obvious.
    #[test]
    fn a_vault_destination_writes_into_a_notes_subtree_and_never_into_the_recordings_folder() {
        let root = stub_temp_root("rec-42-4-vault");
        let recordings = root.join("recordings");
        std::fs::create_dir_all(&recordings).expect("recordings root");
        let folder = recordings.join("keeper-rec session");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, Some("Weekly sync"));
        sink.manifest
            .set_ended_at("2026-01-02T11:00:00+02:00".to_owned());

        let vault = stub_vault(&root);
        let dest = stub_destination(&recordings, Some(vault.clone()));
        write_recording_note_stub(&sink.manifest, &dest);

        let landed = vault
            .root
            .join("recordings")
            .join("2026-01-02-weekly-sync.md");
        assert!(landed.is_file(), "the stub is in a subtree of the vault");
        assert!(
            markdown_names(&recordings).is_empty(),
            "a note was written into the recordings folder the profile refuses to overlap"
        );

        let source = std::fs::read_to_string(&landed).expect("the stub");
        let (fm, _) = Frontmatter::parse(&source);
        assert_eq!(
            fm.as_string("recording"),
            Some("recordings/keeper-rec session"),
            "inside a vault, the recording is named relative to the synced folder — the unit \
             that gets cloned to the other machine"
        );
        assert!(
            !source.contains(&root.to_string_lossy().into_owned()),
            "the profile root reached the stub"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The plain-folder branch: beside the session folder, meaning in its parent
    /// — which is also why two sessions can collide there at all.
    #[test]
    fn a_plain_folder_destination_writes_the_stub_beside_the_session_folder() {
        let root = stub_temp_root("rec-42-4-beside");
        let folder = root.join("keeper-rec session");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, Some("Weekly sync"));
        sink.manifest
            .set_ended_at("2026-01-02T11:00:00+02:00".to_owned());

        let dest = stub_destination(&root, None);
        assert_eq!(dest.dir, root, "beside means the parent, not inside");
        write_recording_note_stub(&sink.manifest, &dest);

        assert!(root.join("2026-01-02-weekly-sync.md").is_file());
        assert!(
            markdown_names(&folder).is_empty(),
            "the stub went inside the session folder rather than beside it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The untitled row, end to end: a date for a title and never a blank
    /// heading, with the filename saying the date once rather than twice.
    #[test]
    fn an_untitled_session_gets_a_dated_stub_with_a_real_heading() {
        let root = stub_temp_root("rec-42-4-untitled");
        let folder = root.join("keeper-rec 2026-01-02 10.00.00");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, None);

        drive_synthetic_session(&mut sink, 1);

        let source = std::fs::read_to_string(root.join("2026-01-02-untitled.md"))
            .expect("an untitled session still gets a named stub");
        let (fm, body) = Frontmatter::parse(&source);
        assert_eq!(fm.as_string("title"), Some("2026-01-02"));
        // The whole body, not a substring: the heading must stay the FIRST line,
        // because `notes_vault::note_title` falls back to it and an embed above
        // it would become the note's displayed name (story 44.2). The embed is
        // here because `drive_synthetic_session` closes a real `.mov`.
        assert_eq!(
            &source[body..],
            "\n# 2026-01-02\n\n![[keeper-rec 2026-01-02 10.00.00/screen-0000.mov]]\n\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Dismiss deletes a stub nobody touched. `false` afterwards, because there
    /// is nothing left to dismiss — the surface treats that as "already gone",
    /// never as a failure.
    #[test]
    fn dismissing_an_untouched_stub_leaves_no_file() {
        let root = stub_temp_root("rec-42-4-dismiss-clean");
        let folder = root.join("keeper-rec session");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, Some("Weekly sync"));
        drive_synthetic_session(&mut sink, 1);

        let lookup = stub_lookup(&folder)
            .expect("the manifest loads")
            .expect("the session has an identity");
        let path = locate_stub(&folder, &lookup.dest, &lookup.session_id).expect("the stub");

        assert!(dismiss_stub(&lookup, &path), "an untouched stub is deleted");
        assert!(!path.exists(), "AC3: no file remains");
        assert!(
            markdown_names(&root).is_empty(),
            "and nothing was left behind under another name — no trash copy"
        );
        assert!(
            locate_stub(&folder, &lookup.dest, &lookup.session_id).is_none(),
            "a second dismissal has nothing to find"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The mistake this whole mechanism exists to make impossible. Every edit
    /// below is one byte or one line away from what keeper composed, because a
    /// check that only caught wholesale rewrites would not be a check.
    #[test]
    fn dismissing_a_stub_the_user_edited_keeps_the_file() {
        let root = stub_temp_root("rec-42-4-dismiss-edited");
        let folder = root.join("keeper-rec session");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, Some("Weekly sync"));
        drive_synthetic_session(&mut sink, 1);

        let lookup = stub_lookup(&folder)
            .expect("the manifest loads")
            .expect("the session has an identity");
        let path = locate_stub(&folder, &lookup.dest, &lookup.session_id).expect("the stub");
        let composed = std::fs::read_to_string(&path).expect("the stub");

        for edit in [
            format!("{composed}Ship on Friday.\n"),
            format!("{composed} "),
            composed.replace("# Weekly sync", "# Weekly sync (moved)"),
        ] {
            std::fs::write(&path, &edit).expect("the user types");
            assert!(
                !dismiss_stub(&lookup, &path),
                "an edited stub was deleted: {edit:?}"
            );
            assert_eq!(
                std::fs::read_to_string(&path).expect("still there"),
                edit,
                "and it must be left exactly as they left it"
            );
        }

        // The other direction, so the test above cannot be passing by refusing
        // everything: restored to the composed bytes, it becomes deletable again.
        std::fs::write(&path, &composed).expect("restore");
        assert!(dismiss_stub(&lookup, &path));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The matrix's edit-then-save row, through the commands the surface calls.
    /// Also the wiring proof: the getter, the writer and the dismisser agree
    /// about which file they are all talking about.
    #[tokio::test]
    async fn the_stub_commands_read_save_and_then_refuse_to_delete_what_was_saved() {
        let root = stub_temp_root("rec-42-4-commands");
        let folder = root.join("keeper-rec session");
        let mut sink = note_stub_sink(&folder, STUB_SESSION_A, Some("Weekly sync"));
        drive_synthetic_session(&mut sink, 1);
        let folder_arg = folder.to_string_lossy().into_owned();

        let stub = recording_note_stub(folder_arg.clone())
            .await
            .expect("the getter succeeds")
            .expect("a stub is waiting");
        assert_eq!(stub.session_id, STUB_SESSION_A);
        assert_eq!(stub.filename, "2026-01-02-weekly-sync.md");
        assert_eq!(stub.relative_path, "2026-01-02-weekly-sync.md");
        assert!(!stub.in_vault);
        // The offset splits the file the way the surface splits it: block on one
        // side, prose on the other, and no way to type into the block. Sliced in
        // UTF-16 exactly as JavaScript would, so an offset that landed inside a
        // surrogate pair fails here rather than in a textarea.
        let units: Vec<u16> = stub.contents.encode_utf16().collect();
        let head = String::from_utf16(&units[..stub.body_offset as usize])
            .expect("the split lands on a character boundary");
        assert!(
            head.ends_with("---\n\n"),
            "the head is keeper's block plus its separator: {head:?}"
        );
        let body = &stub.contents[head.len()..];
        // Story 44.2: the one closed segment is embedded under the heading, and
        // the blank line after it is the line the surface's caret lands on —
        // `setSelectionRange(value.length, …)` on exactly this string. Which is
        // why the sentence appended below goes UNDER the recording rather than
        // shoving it down the page.
        assert_eq!(
            body, "# Weekly sync\n\n![[keeper-rec session/screen-0000.mov]]\n\n",
            "the WHOLE body: heading first, then the recording, then the blank line the \
             caret lands on. An embed above the heading would become the note's title"
        );

        recording_note_stub_save(folder_arg.clone(), format!("{head}{body}Ship on Friday.\n"))
            .await
            .expect("saving the user's words succeeds");

        let saved = recording_note_stub(folder_arg.clone())
            .await
            .expect("the getter succeeds")
            .expect("the stub is still there");
        assert!(
            saved.contents.ends_with("Ship on Friday.\n"),
            "the getter returns what is on disk, not what would be composed"
        );

        assert!(
            !recording_note_stub_dismiss(folder_arg)
                .await
                .expect("dismissing succeeds"),
            "a saved note is kept"
        );
        assert!(
            root.join("2026-01-02-weekly-sync.md").is_file(),
            "and it is still on disk"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The matrix's durability row: the column follows 41.6's floor, and follows
    /// it exactly once per rung. A ~1 Hz poll of a settled session must not write
    /// 3600 identical updates an hour — the send site sits inside the one `if`
    /// that assigns the floor, so "the state changed" and "a write happens" are
    /// the same event by construction.
    #[test]
    fn a_durability_climb_writes_once_per_rung_and_a_repeated_reading_writes_nothing() {
        let folder = scan_temp_dir("rec-42-1-durability");
        let spy = Arc::new(ArchiveSpy::default());
        let archive = archive_session(spy.clone(), &profile_destination(&folder));
        let port = Arc::new(CountingSyncPort::scripted(vec![
            facts(false, false, false),
            facts(false, false, false),
            facts(true, false, false),
            facts(true, false, false),
            facts(true, true, false),
            facts(true, true, false),
            facts(true, true, true),
            facts(true, true, true),
        ]));
        let slot = durability_slot_indexed(&folder, port, Some(archive));

        for _ in 0..8 {
            poll_durability(&slot);
        }

        let updates = spy.durability_updates();
        assert_eq!(
            updates.iter().map(|(_, state)| *state).collect::<Vec<_>>(),
            vec![
                RecordingDurabilityState::Committed,
                RecordingDurabilityState::Pushed,
                RecordingDurabilityState::Verified,
            ],
            "one update per rung the floor actually climbed, and nothing for the \
             four polls that observed a state the session was already at"
        );
        assert!(
            updates
                .iter()
                .all(|(session_id, _)| session_id == INDEXED_SESSION_ID),
            "an update named a session other than its own"
        );
        assert_eq!(
            spy.writes().len(),
            3,
            "reading durability inserted or completed something"
        );

        // The floor's own rule, seen from the archive: an engine answer that drops
        // back is not a move, so it is not a write.
        let sliding = Arc::new(ArchiveSpy::default());
        let slid = archive_session(sliding.clone(), &profile_destination(&folder));
        let port = Arc::new(CountingSyncPort::scripted(vec![
            facts(true, true, false),
            facts(true, false, false),
            facts(false, false, false),
        ]));
        let slot = durability_slot_indexed(&folder, port, Some(slid));
        for _ in 0..3 {
            poll_durability(&slot);
        }
        assert_eq!(
            sliding
                .durability_updates()
                .iter()
                .map(|(_, state)| *state)
                .collect::<Vec<_>>(),
            vec![RecordingDurabilityState::Pushed],
            "a regressing engine answer was written as an advance"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The archive is NOT sync (the epic's whole point): a plain folder publishes
    /// nothing and asks no engine anything, and is indexed exactly as completely as
    /// a profile — with `root_kind` saying which kind of place it is, and no
    /// profile named, because there is none.
    #[test]
    fn a_plain_folder_destination_is_indexed_as_a_folder() {
        let root = scan_temp_dir("rec-42-1-plain-folder");
        let folder = root.join("keeper-rec session");
        let spy = Arc::new(ArchiveSpy::default());
        let archive = archive_session(spy.clone(), &folder_destination(&root));
        let port = Arc::new(CountingSyncPort::new(SessionPushPolicy::PerSegment));
        assert!(
            begin_recording_sync(&folder_destination(&root), "mov", Some(port.clone())).is_none(),
            "a plain folder has no profile to open a sync seam onto"
        );

        let mut sink = recording_sink_indexed(&folder, None, Some(archive.clone()));
        archive.started(&sink.manifest);
        drive_synthetic_session(&mut sink, 12);

        assert_eq!(spy.starts().len(), 1, "a plain-folder session is indexed");
        assert_eq!(spy.segments().len(), 12);
        assert_eq!(spy.completions().len(), 1);
        for row in spy.starts().iter().chain(spy.completions().iter()) {
            assert_eq!(
                row.root_kind, "folder",
                "the row must say which kind of place it is under"
            );
            assert_eq!(
                row.profile_id, None,
                "a plain folder invented a destination profile"
            );
            assert_eq!(row.relative_path, "keeper-rec session");
        }
        assert!(
            port.calls().is_empty(),
            "indexing asked the sync engine something: {:?}",
            port.calls()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The epic's posture, applied to the index (NFR-34 by analogy with 41.5's
    /// refusing engine): an archive whose every write goes nowhere costs the
    /// recording nothing. Not one ledger line, not the finalize, not the manifest,
    /// not the snapshot — and every send site still ran, so the moment the writer
    /// is back the next session indexes normally.
    #[test]
    fn an_archive_that_drops_every_write_costs_the_recording_nothing() {
        let root = scan_temp_dir("rec-42-1-dropping");
        let folder = root.join("keeper-rec session");
        let dropping = Arc::new(DroppingArchive::default());
        let archive = archive_session(dropping.clone(), &profile_destination(&root));
        let port = Arc::new(CountingSyncPort::new(SessionPushPolicy::AtSessionEnd));
        let sync = begin_recording_sync(&profile_destination(&root), "mov", Some(port))
            .expect("the sync seam");

        let mut sink = recording_sink_indexed(&folder, Some(sync), Some(archive.clone()));
        archive.started(&sink.manifest);
        drive_synthetic_session(&mut sink, 48);

        assert_eq!(
            sink.manifest.segments.len(),
            48,
            "a dropped index write cost a ledger line"
        );
        let written = SessionManifest::load(&folder).expect("the finalized manifest");
        assert_eq!(written.segments.len(), 48);
        assert!(matches!(written.status, ManifestStatus::Finalized));
        assert_eq!(
            status_lock(&sink.status).state,
            RecordingUiState::Finalized,
            "a dropped index write surfaced as a failed session"
        );
        assert_eq!(
            dropping.attempts(),
            50,
            "one start, 48 segments and one completion were all still attempted"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A session recording outside its own destination root cannot be described
    /// relative to it, and the rule is absolute: no column may hold an absolute
    /// path. So the row is declined rather than written wrong — and the recording
    /// is, as ever, untouched.
    #[test]
    fn a_session_outside_the_destination_root_is_not_indexed_with_an_absolute_path() {
        let root = scan_temp_dir("rec-42-1-outside");
        let destination_root = root.join("recordings");
        std::fs::create_dir_all(&destination_root).expect("destination root");
        let folder = root.join("elsewhere");
        let spy = Arc::new(ArchiveSpy::default());
        let archive = archive_session(spy.clone(), &profile_destination(&destination_root));

        let mut sink = recording_sink_indexed(&folder, None, Some(archive.clone()));
        archive.started(&sink.manifest);
        drive_synthetic_session(&mut sink, 3);

        assert!(
            spy.writes().is_empty(),
            "a path the index cannot make relative was written anyway: {:?}",
            spy.writes()
        );
        let written = SessionManifest::load(&folder).expect("the finalized manifest");
        assert_eq!(written.segments.len(), 3, "the recording is unaffected");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The matrix's retitle row (Story 42.1, row 11): a Story 40.4 rename moves
    /// the folder, and the row has to follow it. `session_id` is what makes that
    /// possible — the row is keyed on the session's identity, not its location —
    /// so the move sends the new path and nothing else.
    #[test]
    fn a_retitle_repoints_the_row_at_the_new_folder_and_leaves_the_identity_alone() {
        let root = scan_temp_dir("rec-42-1-retitle");
        let moved = root.join("2026").join("2026-01-02 1000 Retro");
        // Only the template's year level: `create_with_meta` creates the session
        // folder itself and refuses one that already exists.
        std::fs::create_dir_all(root.join("2026")).expect("the template's year level");
        let manifest = SessionManifest::create_with_meta(
            moved.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(indexed_meta()),
            Some(INDEXED_STARTED_AT.to_owned()),
        )
        .expect("the retitled session's manifest");
        let spy = Arc::new(ArchiveSpy::default());

        index_retitled_session_on(spy.as_ref(), &root, &moved, &manifest);

        assert_eq!(
            spy.moves(),
            vec![(
                INDEXED_SESSION_ID.to_owned(),
                "2026/2026-01-02 1000 Retro".to_owned()
            )],
            "a retitle sends exactly one repoint, keyed on the session's identity"
        );
        assert!(
            spy.starts().is_empty() && spy.completions().is_empty(),
            "a retitle rewrote the whole row instead of just its path: {:?}",
            spy.writes()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A session recorded before Story 40.3 has no minted identity, so its row
    /// was keyed on the path it had then. The repoint derives the SAME fallback
    /// rule from the new path — which is why it names a different key, and why
    /// `fallback_session_id` documents that a legacy session's row stays behind.
    /// Pinned rather than left implicit: the alternative reading, that a legacy
    /// retitle silently repoints some other session's row, would be a bug.
    #[test]
    fn a_pre_identity_session_repoints_under_the_documented_legacy_fallback_key() {
        let root = scan_temp_dir("rec-42-1-retitle-legacy");
        let moved = root.join("2026").join("2026-01-02 1000 Retro");
        std::fs::create_dir_all(root.join("2026")).expect("the template's year level");
        let manifest = SessionManifest::create_with_meta(
            moved.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            None,
            Some(INDEXED_STARTED_AT.to_owned()),
        )
        .expect("a session with no minted identity");
        let spy = Arc::new(ArchiveSpy::default());

        index_retitled_session_on(spy.as_ref(), &root, &moved, &manifest);

        assert_eq!(
            spy.moves(),
            vec![(
                "legacy:2026/2026-01-02 1000 Retro".to_owned(),
                "2026/2026-01-02 1000 Retro".to_owned()
            )],
            "the legacy key is derived from the path, with the prefix no minted id can produce"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// No column may hold an absolute path, so a session that cannot be expressed
    /// relative to the destination root is not repointed at all — the same
    /// refusal the start and segment paths make, at the one other site that
    /// writes a path.
    #[test]
    fn a_retitle_outside_the_destination_root_repoints_nothing() {
        let root = scan_temp_dir("rec-42-1-retitle-outside");
        let destination_root = root.join("destination");
        let moved = root.join("elsewhere");
        std::fs::create_dir_all(&destination_root).expect("the destination root");
        let manifest = SessionManifest::create_with_meta(
            moved.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(indexed_meta()),
            Some(INDEXED_STARTED_AT.to_owned()),
        )
        .expect("the manifest");
        let spy = Arc::new(ArchiveSpy::default());

        index_retitled_session_on(spy.as_ref(), &destination_root, &moved, &manifest);

        assert!(
            spy.writes().is_empty(),
            "a path the index cannot make relative was repointed anyway: {:?}",
            spy.writes()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- the recordings browser (Story 42.3) --------------------------------

    /// Index one session, with one 4 KiB screen segment, into a real
    /// `archive.db` under `data_dir`.
    fn index_browsable_session(data_dir: &Path, session_id: &str, title: &str) {
        let conn = keeper_core::archive::db::open_archive_db(data_dir).expect("open the archive");
        let row = keeper_core::archive::RecordingRow {
            session_id: session_id.to_owned(),
            device_id: None,
            relative_path: format!("2026/{title}"),
            root_kind: "folder".to_owned(),
            profile_id: None,
            started_ts: Some(1_000),
            ended_ts: Some(61_000),
            title: Some(title.to_owned()),
            participants_json: None,
            note: None,
            tags_json: None,
            custom_json: None,
            codec: None,
            width: None,
            height: None,
            fps: None,
            durability: "local".to_owned(),
            manifest_version: 1,
        };
        keeper_core::archive::recordings::upsert_recording(&conn, &row).expect("index the session");
        keeper_core::archive::recordings::upsert_segment(
            &conn,
            &keeper_core::archive::RecordingSegmentRow {
                session_id: session_id.to_owned(),
                index: 0,
                track: "screen".to_owned(),
                relative_path: format!("2026/{title}/screen-0000.mov"),
                bytes: 4_096,
                pts_start: None,
                pts_end: None,
                closed_ts: None,
            },
        )
        .expect("index the segment");
    }

    /// Story 42.3, matrix row 1: a machine that has never recorded — or never
    /// synced — has no `archive.db` at all, and browsing it is an empty list,
    /// not the `SQLITE_CANTOPEN` a read-only open of a missing file would
    /// raise. This is `search_archive`'s rule and the first thing a fresh
    /// install does.
    #[test]
    fn browsing_recordings_with_no_archive_yields_no_rows_and_no_error() {
        let data_dir = scan_temp_dir("rec-42-3-no-archive");

        let found = search_recordings_in(
            &data_dir,
            &data_dir.join("Movies"),
            RecordingFilterVm {
                query: String::new(),
                tags: Vec::new(),
                participant: None,
                start_ts: None,
                end_ts: None,
                durability: None,
                profile_id: None,
                limit: None,
            },
        )
        .expect("an absent archive is an empty answer, never an error");

        assert!(found.rows.is_empty());
        assert_eq!(
            found.total, 0,
            "no archive is zero sessions, said as a number (Story 44.11)"
        );
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// Story 42.3: with an archive, the command answers the filter it was given
    /// and resolves each row against the destination root the shell passed in —
    /// the row's Reveal target, composed in Rust and nowhere else (AD-65).
    #[test]
    fn browsing_recordings_returns_rows_resolved_against_the_destination_root() {
        let base = scan_temp_dir("rec-42-3-browse");
        let data_dir = base.join("data");
        let destination_root = base.join("Movies").join("keeper");
        index_browsable_session(&data_dir, "01DEVICE-01STANDUP", "Standup");
        let filter = |query: &str| RecordingFilterVm {
            query: query.to_owned(),
            tags: Vec::new(),
            participant: None,
            start_ts: None,
            end_ts: None,
            durability: None,
            profile_id: None,
            limit: None,
        };

        let rows = search_recordings_in(&data_dir, &destination_root, filter("standup"))
            .expect("browse the archive");
        let missing = search_recordings_in(&data_dir, &destination_root, filter("retrospective"))
            .expect("browse the archive");

        assert_eq!(rows.rows.len(), 1);
        assert_eq!(
            rows.total, 1,
            "the count travels with the page (Story 44.11)"
        );
        assert_eq!(rows.rows[0].session_id, "01DEVICE-01STANDUP");
        assert_eq!(rows.rows[0].duration_ms, Some(60_000));
        assert_eq!(rows.rows[0].total_bytes, 4_096);
        let expected_folder = destination_root.join("2026").join("Standup");
        let expected_file = expected_folder
            .join("screen-0000.mov")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            rows.rows[0].absolute_path,
            expected_folder.to_string_lossy()
        );
        assert_eq!(
            rows.rows[0].playable_path.as_deref(),
            Some(expected_file.as_str())
        );
        assert!(
            missing.rows.is_empty(),
            "a filter that matches nothing is an empty list, not an error"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    // --- a recording note's file actions (Story 42.4) -----------------------

    /// Story 42.4: the note's reader gets the session folder and every file in
    /// it, each resolved against the destination root the shell passed in —
    /// the only place a root and a subfolder are ever joined (AD-65) — and
    /// only a video is marked as something Preview can open.
    #[test]
    fn a_recording_note_resolves_its_session_folder_and_files_against_the_destination_root() {
        use keeper_core::vm::RecordingNoteTargetKind;

        let base = scan_temp_dir("rec-42-4-targets");
        let data_dir = base.join("data");
        let destination_root = base.join("Movies").join("keeper");
        index_browsable_session(&data_dir, "01DEVICE-01STANDUP", "Standup");
        let folder = destination_root.join("2026").join("Standup");
        std::fs::create_dir_all(&folder).expect("the session folder");
        for name in ["screen-0000.mov", "manifest.json"] {
            std::fs::write(folder.join(name), b"bytes").expect("a session file");
        }

        let targets = recording_note_targets_in(&data_dir, &destination_root, "01DEVICE-01STANDUP")
            .expect("a known session resolves")
            .expect("a session on disk has targets");

        assert_eq!(
            targets
                .iter()
                .map(|target| (target.relative_path.as_str(), target.kind))
                .collect::<Vec<_>>(),
            vec![
                ("2026/Standup", RecordingNoteTargetKind::Folder),
                ("2026/Standup/manifest.json", RecordingNoteTargetKind::File),
                (
                    "2026/Standup/screen-0000.mov",
                    RecordingNoteTargetKind::Video
                ),
            ]
        );
        assert_eq!(
            targets[0].absolute_path,
            folder.to_string_lossy(),
            "Reveal opens the folder the recorder wrote to, composed here and nowhere else"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Story 42.4: a note can outlive the archive that knows its session — a
    /// fresh install syncing an old vault has the note and no `archive.db` at
    /// all. That is `None`, not `SQLITE_CANTOPEN`, so the surface renders the
    /// note's own text and offers nothing that would open nothing.
    #[test]
    fn a_recording_note_on_a_machine_with_no_archive_has_no_targets_and_no_error() {
        let data_dir = scan_temp_dir("rec-42-4-no-archive");

        let targets =
            recording_note_targets_in(&data_dir, &data_dir.join("Movies"), "01DEVICE-01STANDUP")
                .expect("an absent archive is an empty answer, never an error");

        assert_eq!(targets, None);
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// Story 42.3, the security-relevant one: a command that opened whatever
    /// path the webview named would launch the user's default application on
    /// any readable file. Every way out of the recordings root is refused —
    /// a sibling directory, a `..` walk that lexically strips clean, and a
    /// symlink inside the root that no string test can catch — and the refusal
    /// is non-retriable, because retrying will not make it contained.
    #[cfg(desktop)]
    #[test]
    fn opening_a_recording_refuses_every_path_outside_the_destination_root() {
        let base = scan_temp_dir("rec-42-3-containment");
        let root = base.join("Movies").join("keeper");
        let session = root.join("2026").join("Standup");
        std::fs::create_dir_all(&session).expect("the session folder");
        let inside = session.join("screen-0000.mov");
        std::fs::write(&inside, b"bytes").expect("the segment");
        let secret = base.join("secret.txt");
        std::fs::write(&secret, b"private").expect("the secret");

        // The honest case still works, and resolves to the file itself.
        assert_eq!(
            contained_recording_path(&root, &inside.to_string_lossy())
                .expect("a file inside the root is opened"),
            inside.canonicalize().expect("canonical segment")
        );

        for outside in [
            secret.to_string_lossy().into_owned(),
            root.join("..")
                .join("secret.txt")
                .to_string_lossy()
                .into_owned(),
            session
                .join("..")
                .join("..")
                .join("..")
                .join("secret.txt")
                .to_string_lossy()
                .into_owned(),
            "/etc/passwd".to_owned(),
            root.to_string_lossy().into_owned(),
        ] {
            let refusal = contained_recording_path(&root, &outside)
                .expect_err(&format!("{outside} was opened from outside the root"));
            assert_eq!(refusal.code, IpcErrorCode::Internal);
            assert!(!refusal.retriable);
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Story 42.3: the lexical half of the check cannot see a symlink, so the
    /// canonicalizing half is what actually refuses one — a link planted inside
    /// the recordings folder pointing at a file outside it passes every string
    /// test there is.
    #[cfg(all(desktop, unix))]
    #[test]
    fn opening_a_recording_refuses_a_symlink_that_escapes_the_destination_root() {
        let base = scan_temp_dir("rec-42-3-symlink");
        let root = base.join("Movies").join("keeper");
        std::fs::create_dir_all(&root).expect("the root");
        let secret = base.join("secret.txt");
        std::fs::write(&secret, b"private").expect("the secret");
        let link = root.join("escape.mov");
        std::os::unix::fs::symlink(&secret, &link).expect("the planted symlink");

        let refusal = contained_recording_path(&root, &link.to_string_lossy())
            .expect_err("a symlink out of the root was followed");

        assert_eq!(refusal.code, IpcErrorCode::Internal);
        assert!(!refusal.retriable);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Story 42.3, matrix row 8: a session whose folder is gone — moved or
    /// deleted outside keeper — is an honest refusal rather than a path handed
    /// to the opener to fail on in its own words.
    #[cfg(desktop)]
    #[test]
    fn opening_a_recording_that_no_longer_exists_is_refused_honestly() {
        let base = scan_temp_dir("rec-42-3-gone");
        let root = base.join("Movies").join("keeper");
        std::fs::create_dir_all(&root).expect("the root");

        let refusal = contained_recording_path(&root, &root.join("gone.mov").to_string_lossy())
            .expect_err("a path that does not resolve was opened");

        assert_eq!(refusal.code, IpcErrorCode::Internal);
        assert!(!refusal.retriable);
        let _ = std::fs::remove_dir_all(&base);
    }
}
