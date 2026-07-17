//! IPC command layer for the keeper shell (AD-8, AD-21).
//!
//! This is the place where [`CoreError`] is mapped to the `IpcError` envelope,
//! where the bulk of the `#[tauri::command]`s live, and where the concrete
//! [`Platform`] port is implemented. The app-lifecycle command is the one
//! deliberate peer seam — it lives in [`crate::lifecycle`] (Epic 14-1) so the
//! single Rust lifecycle entry point is self-contained. No business logic lives
//! in either module — commands delegate to `keeper-core`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_core::account::AccountManager;
use keeper_core::auth;
use keeper_core::auth::BeeperFlowRegistry;
use keeper_core::demo::snapshot_then_diff;
use keeper_core::egress::{compute_egress, EGRESS_UPDATE_ENDPOINT};
use keeper_core::error::{
    AccountError, ArchiveError, AuthError, BackupError, BridgeError, CoreError, InboxError,
    MediaError, PlatformError, SendError, SignalError, TimelineError, VerificationError,
};
use keeper_core::oauth::OAuthFlowRegistry;
use keeper_core::platform::Platform;
use keeper_core::recording::{
    resolve_screen_recording_access, session_folder_name, CaptureTarget, ManifestStatus, Recorder,
    RecordingEvent, RecordingSession, SegmentEntry, SessionDevices, SessionManifest, SessionParams,
    SessionState,
};
use keeper_core::vm::{
    AccountVm, ApprovalDraftVm, BackupStatus, BbctlAvailabilityVm, BbctlProgressVm,
    BridgeDiscoveryVm, BridgeHealthSnapshot, BridgeLoginInput, BridgeLoginVm, BridgeNetworkVm,
    CapabilitiesVm, ChatNotifyMode, ConnectionStatusBatch, CouplingCaveatVm, DemoBatch,
    DockBadgeMode, DraftMirrorBatch, EditVersionVm, EgressEndpointVm, EncryptionStatusBatch,
    ExportPhase, ExportProgressVm, ExportRequestVm, HotkeyVm, InboxBatch, IncognitoVm, IpcError,
    IpcErrorCode, MenuSectionVm, NavState, NetworksSnapshot, NewChatResolutionVm,
    NotificationPermission, NotifyTarget, OutboxVm, PaginationStatusBatch, PaletteMode,
    PaletteResultsVm, PingVm, Provider, RecordingPermissionVm, RecordingStatusVm, RecordingUiState,
    RemoteDraftVm, ResolveSupportVm, RoomListBatch, ScreenRecordingAccess, SearchFilterVm,
    SearchHitVm, SpacesSnapshot, TccPermission, TimelineBatch, TypingBatch, VerificationFlowVm,
};
use tauri::ipc::Channel;
use tauri::State;

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
    /// clears it. All `api.beeper.com` HTTP is confined to `keeper-core`.
    pub beeper_flows: Arc<BeeperFlowRegistry>,
    /// Live archive-export jobs (Story 5.5). Maps each `exportId` to its shared
    /// `Arc<AtomicBool>` cancel flag: `export_start` registers a flag before
    /// spawning the blocking job, `export_cancel` sets it, and the job deregisters
    /// itself on any terminal phase. The `AtomicU64` mints monotonic ids.
    pub exports: Arc<ExportRegistry>,
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
            bbctl_runs: Arc::new(BbctlRunRegistry::default()),
            paused_at: Mutex::new(None),
            nav_state: Mutex::new(None),
            recorder,
            recording_permission_requested: AtomicBool::new(false),
            recording_run: Mutex::new(None),
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
#[cfg(desktop)]
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
#[cfg(desktop)]
pub const NOTIFY_NAVIGATE_EVENT: &str = "notify://navigate";

/// Emit the coarse navigate event to the main window from the last recorded notification
/// target (Story 10.4, Option B), then reset the target so it fires once per notification.
///
/// A [`NotifyTarget::None`] (no notification since the last activation, e.g. a plain
/// dock-click) is a no-op — only Message/Bridge targets emit. Best-effort: a missing
/// window or an emit failure is logged at `warn`, never a panic.
#[cfg(desktop)]
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
        entry
            .set_password(value)
            .map_err(|e| PlatformError::Keychain(format!("could not store secret: {e}")))?;
        Ok(())
    }

    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .map_err(|e| PlatformError::Keychain(format!("could not open keychain entry: {e}")))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(PlatformError::Keychain(format!("could not read secret: {e}")).into()),
        }
    }

    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .map_err(|e| PlatformError::Keychain(format!("could not open keychain entry: {e}")))?;
        match entry.delete_credential() {
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
fn to_ipc_error(err: CoreError) -> IpcError {
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
        // Recording errors (Story 16.2) do not cross the IPC command surface in this
        // story — the recording session machine and its `keeper-rec` port are driven
        // shell-side, not from a command. This arm keeps the funnel exhaustive; a
        // dedicated recording IPC surface (with its own honest codes) arrives in a
        // later recording story (16.3+). Until then, an internal, non-retriable error.
        CoreError::Recording(_) => (IpcErrorCode::Internal, false),
    };
    IpcError {
        code,
        message: err.to_string(),
        account_id: None,
        retriable,
    }
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
pub fn capabilities() -> Result<CapabilitiesVm, IpcError> {
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
    })
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

/// Cancel any in-progress Beeper login flow(s) (Story 2.3). Clears the registry
/// so no pending request id lingers; nothing is persisted. Idempotent — with no
/// pending flow it is a no-op.
#[tauri::command]
pub fn cancel_beeper(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.beeper_flows.cancel_all();
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
    // (and thus the cheat sheet + native menu) when unavailable (Story 16.3).
    let recording = crate::macos_version::recording_supported();
    Ok(state
        .accounts
        .palette_query(&query, mode, open_chat, recording)
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
pub fn cheat_sheet_sections() -> Result<Vec<MenuSectionVm>, IpcError> {
    // Gate the recording action out of the cheat sheet when the capability is off
    // (Story 16.3), keeping it in lockstep with the palette and native menu.
    Ok(keeper_core::palette::registry_sections(
        crate::macos_version::recording_supported(),
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

/// Project a resolved Screen Recording tri-state into the [`RecordingPermissionVm`]
/// the Recording view renders (Story 16.5, FR-67). `can_start` is `true` only when
/// the grant is green — Screen Recording is the sole required permission this epic.
fn recording_permission_vm(access: ScreenRecordingAccess) -> RecordingPermissionVm {
    RecordingPermissionVm {
        screen_recording: access,
        can_start: access == ScreenRecordingAccess::Granted,
    }
}

/// Resolve the live Screen Recording permission pre-flight (Story 16.5, FR-67,
/// AD-36). Runs the sidecar `getCapabilities` probe (a fresh child `keeper-rec`
/// per call — live detection, never a cached grant; bounded by the shell's
/// pre-flight timeout so a wedged sidecar resolves a clean error) and lifts the
/// two-valued preflight into the honest tri-state with the session
/// "already requested" flag via the pure core resolver. Called at Recording-view
/// render and re-called on every focus/return. Failures (sidecar unavailable /
/// hung / iOS) funnel through [`to_ipc_error`]; the frontend swallows them to a
/// safe default (Start disabled) — never a crash, never an infinite spinner.
#[tauri::command]
pub async fn recording_permission(
    state: State<'_, AppState>,
) -> Result<RecordingPermissionVm, IpcError> {
    let capabilities = state
        .recorder
        .get_capabilities()
        .await
        .map_err(to_ipc_error)?;
    let requested = state.recording_permission_requested.load(Ordering::Relaxed);
    Ok(recording_permission_vm(resolve_screen_recording_access(
        capabilities.screen_recording,
        requested,
    )))
}

/// Request Screen Recording access through the sidecar (Story 16.5, FR-67,
/// AD-36): sets the session "already requested" flag, runs the
/// `requestScreenRecording` round-trip (`CGRequestScreenCaptureAccess` in the
/// child sidecar, so TCC shows keeper's own usage string and the OS posts its one
/// real prompt per app lifetime where allowed), and re-resolves the tri-state
/// from the reported outcome: granted ⇒ Start unlocks; not granted (a prior
/// denial shows no prompt at all) ⇒ denied-with-fix-path, and the row offers the
/// System Settings deep link. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn request_screen_recording_permission(
    state: State<'_, AppState>,
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
    Ok(recording_permission_vm(resolve_screen_recording_access(
        preflight, true,
    )))
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

/// The current Unix epoch in milliseconds (0 on a pre-1970 clock — never a panic).
fn epoch_ms_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Start the (at most one) full-screen + system-audio recording session (Story
/// 16.6 + 17.2, FR-68/FR-69/FR-71, AD-33/AD-37): create the per-session folder
/// `~/Movies/keeper/keeper-rec <local timestamp>/` with its initial `recording`
/// `manifest.json`, spawn the driver task (fresh `keeper-rec` child; NDJSON
/// events fold through the platform-free session machine into the polled status
/// snapshot AND the segment ledger), and return the initial snapshot. The
/// sidecar writes `screen-0000.mp4` (then `screen-0001.mp4`, … on rotation)
/// inside the folder. A still-live prior session is an honest error — never two
/// capture children. Pre-spawn folder/manifest failures funnel through
/// [`to_ipc_error`] (no session task exists yet); a mid-session manifest-write
/// failure is logged only and never flips the live session to `failed` (the
/// single-child start-guard keys off the snapshot).
#[tauri::command]
pub async fn recording_start(state: State<'_, AppState>) -> Result<RecordingStatusVm, IpcError> {
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

    // Default destination (the Destination card is a later story): the user's
    // Movies folder, falling back to the app data dir where Movies is absent.
    let directory = dirs::video_dir()
        .unwrap_or(state.platform.data_dir().map_err(to_ipc_error)?)
        .join("keeper");
    // The shell supplies the local timestamp (core is time-agnostic); the core
    // derives + validates the fs-safe folder basename (Story 17.2).
    let local_ts = chrono::Local::now().format("%Y-%m-%d %H.%M.%S").to_string();
    let base_name = session_folder_name(&local_ts).map_err(|e| to_ipc_error(e.into()))?;
    // The session folder must be UNIQUE — a same-second sequential restart must
    // never reuse (and cross-write) the prior session's folder. Disambiguate
    // with ` (2)`, ` (3)`, … until a fresh name is found; `SessionManifest::
    // create` additionally refuses an existing directory outright.
    let mut folder = directory.join(&base_name);
    let mut suffix: u32 = 2;
    while folder.exists() {
        folder = directory.join(format!("{base_name} ({suffix})"));
        suffix = suffix.saturating_add(1);
    }
    // Create the folder + the initial `recording` manifest. This synchronous
    // pre-spawn failure is a legitimate command error — no session task exists
    // yet, so surfacing it cannot desync any start-guard.
    let manifest = SessionManifest::create(
        folder.clone(),
        CaptureTarget::display(None),
        SessionDevices {
            system_audio: true,
            microphone: false,
            camera: false,
        },
    )
    .map_err(|e| to_ipc_error(e.into()))?;

    let params = SessionParams {
        // Seeding `screen-0000.mp4` lets 17.1's `nextSegmentPath` produce
        // `screen-0001.mp4`, … inside the folder with no Swift change.
        output_path: folder
            .join("screen-0000.mp4")
            .to_string_lossy()
            .into_owned(),
        display_id: None,
        system_audio: true,
    };
    let status = Arc::new(Mutex::new(RecordingStatusVm {
        state: RecordingUiState::Preflight,
        segments_closed: 0,
        started_at_epoch_ms: None,
        // The session is a folder now (Story 17.2) — the VM points at it.
        output_path: Some(folder.to_string_lossy().into_owned()),
        error: None,
    }));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let recorder = state.recorder.clone();
    let task_status = status.clone();
    tauri::async_runtime::spawn(async move {
        // Fold sidecar events through the platform-free machine into the shared
        // snapshot as they arrive (live — unlike `drive_session`'s buffered replay).
        let mut machine = RecordingSession::new();
        let mut manifest = manifest;
        // Warn at most once per session on a manifest-write failure: a persistent
        // fault (read-only volume, deleted folder) would otherwise spam the log on
        // every event across an hours-long recording. Non-fatal either way.
        let mut manifest_write_warned = false;
        let sink_status = task_status.clone();
        let sink = Box::new(move |event: RecordingEvent| {
            let failure = match &event {
                RecordingEvent::Failed { message } => Some(message.clone()),
                _ => None,
            };
            let started = matches!(event, RecordingEvent::CaptureStarted);
            // Capture the ledger entry before `apply` consumes the event: the
            // basename comes from the sidecar-reported path (synthesized from
            // the index when absent); `bytes`/`track` degrade to 0/"screen".
            // This is only the LIVE view — the terminal reconcile rebuilds the
            // list authoritatively from disk.
            let segment = match &event {
                RecordingEvent::SegmentClosed {
                    index,
                    path,
                    bytes,
                    track,
                } => Some(SegmentEntry {
                    index: *index,
                    file: path
                        .as_deref()
                        .and_then(|p| Path::new(p).file_name())
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("screen-{index:04}.mp4")),
                    bytes: bytes.unwrap_or(0),
                    track: track.clone().unwrap_or_else(|| "screen".to_owned()),
                }),
                _ => None,
            };
            if machine.apply(event).is_ok() {
                {
                    let mut snapshot = status_lock(&sink_status);
                    snapshot.state = recording_ui_state(machine.state());
                    snapshot.segments_closed = machine.segments_closed();
                    if started && snapshot.started_at_epoch_ms.is_none() {
                        snapshot.started_at_epoch_ms = Some(epoch_ms_now());
                    }
                    if let Some(message) = failure {
                        snapshot.error = Some(message);
                    }
                }
                // Segment ledger + manifest (Story 17.2). Best-effort by
                // design: a mid-session write/reconcile failure is LOGGED ONLY
                // — it must never change `machine` state or force the snapshot
                // to `Failed`, because capture is still live and the
                // single-child start-guard keys off the snapshot (a false
                // `Failed` would let a second `keeper-rec` child spawn). The
                // last good manifest stays on disk.
                if let Some(entry) = segment {
                    manifest.record_segment(entry);
                }
                manifest.set_status(ManifestStatus::from_state(machine.state()));
                if matches!(
                    machine.state(),
                    SessionState::Finalized | SessionState::Recovered | SessionState::Failed
                ) {
                    // EVERY terminal rebuilds the segment list from disk —
                    // disk is authoritative (final segment, DW-992 backfill,
                    // real sizes) — before the final write.
                    if let Err(error) = manifest.reconcile_from_dir() {
                        tracing::warn!(
                            %error,
                            "recording manifest: terminal disk reconcile failed; \
                             writing the event-fed view instead"
                        );
                    }
                }
                if let Err(error) = manifest.write() {
                    if !manifest_write_warned {
                        manifest_write_warned = true;
                        tracing::warn!(
                            %error,
                            "recording manifest: atomic write failed; the last good \
                             manifest stays on disk and further write failures this \
                             session are suppressed (session unaffected)"
                        );
                    }
                }
            }
        }) as Box<dyn FnMut(RecordingEvent) + Send>;

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
        // already surface as a terminal event becomes an honest failed snapshot.
        if let Err(error) = outcome {
            let mut snapshot = status_lock(&task_status);
            if !matches!(
                snapshot.state,
                RecordingUiState::Finalized
                    | RecordingUiState::Recovered
                    | RecordingUiState::Failed
            ) {
                snapshot.state = RecordingUiState::Failed;
                snapshot.error = Some(error.to_string());
            }
        }
    });

    let snapshot = status_lock(&status).clone();
    *guard = Some(RecordingRun {
        stop_tx: Some(stop_tx),
        status,
    });
    Ok(snapshot)
}

/// Request a graceful stop of the live recording session (Story 16.6): fires the
/// one-shot stop trigger; the sidecar finalizes the file (`stopping` →
/// `finalized` on the polled snapshot) and exits. Idempotent — a second stop (or
/// a stop after the session ended) is a no-op, never an error, never a kill.
#[tauri::command]
pub fn recording_stop(state: State<'_, AppState>) -> Result<(), IpcError> {
    let mut guard = slot_lock(&state.recording_run);
    if let Some(run) = guard.as_mut() {
        if let Some(tx) = run.stop_tx.take() {
            // A send failure means the driver task already ended — nothing to stop.
            let _ = tx.send(());
        }
    }
    Ok(())
}

/// Read the current recording-session status snapshot (Story 16.6) — the poll
/// the Recording view's active-session UI renders from. No session yet this app
/// lifetime ⇒ the honest idle snapshot. Infallible in practice.
#[tauri::command]
pub fn recording_status(state: State<'_, AppState>) -> Result<RecordingStatusVm, IpcError> {
    let guard = slot_lock(&state.recording_run);
    Ok(guard
        .as_ref()
        .map(|run| status_lock(&run.status).clone())
        .unwrap_or_else(RecordingStatusVm::idle))
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

/// Report the live set of network destinations keeper contacts (Story 11.2,
/// NFR-11, UX-DR17). Reads the accounts registry from the same path
/// [`session_restore`] uses — `registry::list_accounts` — projects each row to its
/// `(homeserver_url, Provider)`, and feeds them plus the shared
/// [`EGRESS_UPDATE_ENDPOINT`] into the pure `compute_egress`. The result is
/// rendered as UI under Settings → About so keeper's egress claim is verifiable,
/// never asserted: each homeserver (deduped), `api.beeper.com` exactly when a
/// Beeper account exists, and the update endpoint. A legacy row with no/unknown
/// `provider` tag maps to [`Provider::Password`] — Beeper detection still catches
/// it by host. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn egress_list(state: State<'_, AppState>) -> Result<Vec<EgressEndpointVm>, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let rows = keeper_core::registry::list_accounts(&data_dir).map_err(to_ipc_error)?;
    let accounts: Vec<(String, Provider)> = rows
        .into_iter()
        .map(|row| {
            // A row created after Story 2.5 carries a durable provider tag; a legacy
            // NULL / unrecognized tag falls back to Password. Beeper-by-host detection
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
    Ok(compute_egress(&accounts, EGRESS_UPDATE_ENDPOINT))
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
}
