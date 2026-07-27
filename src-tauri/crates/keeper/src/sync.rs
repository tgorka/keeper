//! Desktop wiring for the folder-sync engine (Epic 29, AD-40/AD-51).
//!
//! `keeper-sync` deliberately knows nothing about Tauri, so everything
//! platform-shaped lives here: resolving `git`, probing free space, reading the
//! clock, bridging secrets to the Keychain, and owning the supervisor task. The
//! engine holds policy; this module holds the OS.
//!
//! # Availability is a runtime fact, not a build flag
//!
//! The engine cannot exist without a usable `git` (AD-41), so it is built
//! lazily and its absence is reported honestly through `CapabilitiesVm.sync`
//! rather than by shipping surfaces that fail when pressed. A machine with no
//! git simply has no sync UI — the AD-27 "no dead buttons" rule.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_core::platform::Platform;
use keeper_core::vm::NotifyTarget;
use keeper_sync::engine::Engine;
use keeper_sync::{Result as SyncResult, SyncError, SyncPlatform};

/// Bridges the engine's port onto the shell's existing [`Platform`].
///
/// A separate port rather than a reuse of `Platform` because AD-40 keeps
/// `keeper-sync` free of `keeper-core`; this adapter is the seam where the two
/// meet, and it is deliberately the only place that knows both.
pub struct ShellSyncPlatform {
    platform: Arc<dyn Platform>,
    host_label: String,
}

impl ShellSyncPlatform {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self {
            platform,
            host_label: read_host_label(),
        }
    }
}

impl SyncPlatform for ShellSyncPlatform {
    fn data_dir(&self) -> SyncResult<PathBuf> {
        self.platform
            .data_dir()
            .map_err(|err| SyncError::Config(format!("no data directory: {err}")))
    }

    fn secret_get(&self, key: &str) -> SyncResult<Option<String>> {
        self.platform
            .keychain_get(key)
            .map_err(|err| SyncError::Config(format!("keychain read failed: {err}")))
    }

    fn secret_set(&self, key: &str, value: &str) -> SyncResult<()> {
        self.platform
            .keychain_set(key, value)
            .map_err(|err| SyncError::Config(format!("keychain write failed: {err}")))
    }

    fn secret_delete(&self, key: &str) -> SyncResult<()> {
        self.platform
            .keychain_delete(key)
            .map_err(|err| SyncError::Config(format!("keychain delete failed: {err}")))
    }

    fn notify(&self, title: &str, body: &str) {
        // Sync warnings bypass Do Not Disturb for the same reason recording
        // faults do (AD-39): a sync that has stopped needing attention is a
        // loud failure, and the engine already raises this at most once per
        // onset.
        if let Err(err) = self.platform.notify(title, body, &NotifyTarget::None) {
            // Best-effort by contract: a machine with no notifier must not fail
            // a sync. Logged rather than swallowed so a systematically broken
            // notifier is still discoverable.
            tracing::warn!(error = %err, "could not raise a sync notification");
        }
    }

    fn now_ms(&self) -> i64 {
        // Wall clock, deliberately: the scheduler reasons about time that
        // passed while the process was not running, which a monotonic clock
        // cannot express.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or(0)
    }

    fn free_space(&self, path: &Path) -> Option<u64> {
        // Fail-open, matching the recording disk guard: refusing to sync
        // because a statvfs failed is worse than running out of space and
        // saying so.
        fs4::available_space(path).ok()
    }

    fn git_program(&self) -> SyncResult<PathBuf> {
        find_git().ok_or_else(|| SyncError::GitMissing {
            reason: "no `git` on PATH; install git 2.42 or newer to use folder sync".to_owned(),
        })
    }

    fn host_label(&self) -> String {
        self.host_label.clone()
    }
}

/// Locate `git` on `PATH`.
///
/// Resolved by hand rather than with a crate: this is a handful of `join`s and
/// an `is_file` check, and a GUI app's `PATH` is thin enough that the extra
/// well-known locations matter more than any library would.
fn find_git() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("git");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // A macOS app launched from Finder inherits a minimal `PATH` that often
    // omits Homebrew entirely, so a bare PATH search would report "no git" on
    // a machine that plainly has it.
    [
        "/opt/homebrew/bin/git",
        "/usr/local/bin/git",
        "/usr/bin/git",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

/// This machine's short name, for provenance trailers and conflict filenames.
fn read_host_label() -> String {
    let raw = std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default();
    // macOS answers with a Bonjour name (`macbookpro.lan`); the leading label
    // keeps a commit trailer short.
    let short = raw.split('.').next().unwrap_or_default().trim();
    if short.is_empty() {
        "unknown-host".to_owned()
    } else {
        short.to_owned()
    }
}

/// The process-wide engine, built on first use.
///
/// An empty slot rather than a `LazyLock<Engine>` because construction can
/// legitimately fail (no git) and must be **retryable**: a user who installs
/// git should not have to restart the app to get sync back.
static ENGINE: LazyLock<Mutex<Option<Arc<Engine>>>> = LazyLock::new(|| Mutex::new(None));

fn slot() -> MutexGuard<'static, Option<Arc<Engine>>> {
    ENGINE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Get the engine, building it if this is the first call.
///
/// Returns `GitMissing` when the prerequisite is absent, which every caller
/// surfaces rather than papering over.
pub fn engine(platform: Arc<dyn Platform>) -> SyncResult<Arc<Engine>> {
    let mut guard = slot();
    if let Some(existing) = guard.as_ref() {
        return Ok(Arc::clone(existing));
    }
    let built = Arc::new(Engine::open(Arc::new(ShellSyncPlatform::new(platform)))?);
    *guard = Some(Arc::clone(&built));
    Ok(built)
}

/// The engine **only if it has already been built**.
///
/// For callers that must not pay for construction — the ~1 Hz tray tick above
/// all, which cannot afford to open a database, and must not create one as a
/// side effect of painting an icon.
pub fn engine_if_open() -> Option<Arc<Engine>> {
    slot().as_ref().map(Arc::clone)
}

/// The engine's platform port over the shell's [`Platform`], for the callers
/// that need the port itself rather than the engine.
///
/// Only the credential commands use this: a token is something the engine
/// exclusively *reads* (`secret_get`, three call sites), so writing one has no
/// engine-side entry point and would otherwise require inventing an
/// `Engine::set_credential` that the engine has no use for.
pub fn sync_platform(platform: Arc<dyn Platform>) -> ShellSyncPlatform {
    ShellSyncPlatform::new(platform)
}

/// The live supervisor's stop signal, if one is running.
///
/// Holding the sender here (rather than in `AppState`) keeps the whole
/// supervisor lifecycle in the module that owns the engine, and makes
/// [`start_supervisor`] idempotent: a second call sees an occupied slot and
/// returns instead of racing a second loop against the same journal.
static SUPERVISOR: LazyLock<Mutex<Option<tokio::sync::watch::Sender<bool>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Start the 1 Hz sync supervisor for this process (Epic 26/29).
///
/// Without this the app has no background sync at all: the engine exists, the
/// journal fills, and nothing drains it until the user presses "Sync now" —
/// which is how v0.4.x shipped. `keeper-syncd` has always driven the same loop
/// (its `run_supervisor`); this is the app doing the equivalent so a configured
/// folder converges without a separate daemon installed.
///
/// Best-effort and quiet by design. A machine with no `git` has no sync
/// capability and therefore no surface to feed, so a failed engine build is a
/// debug line, not a warning — exactly the honesty rule the capability handshake
/// already follows. Idempotent; safe to call before any profile exists (the
/// tick is a no-op then).
pub fn start_supervisor(platform: Arc<dyn Platform>) {
    let mut guard = supervisor_slot();
    if guard.is_some() {
        return;
    }
    let engine = match engine(platform) {
        Ok(engine) => engine,
        Err(err) => {
            tracing::debug!(%err, "sync: no supervisor (engine unavailable)");
            return;
        }
    };
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    *guard = Some(stop_tx);
    drop(guard);
    tauri::async_runtime::spawn(async move {
        if let Err(err) = engine.run(stop_rx).await {
            tracing::warn!(%err, "sync: supervisor stopped");
        }
    });
}

/// Signal the supervisor to finish its current unit and stop.
///
/// Called on the quit path. The engine's own shutdown is what makes an
/// in-flight push *abort resumably* — its journal row survives and is re-driven
/// next launch — so skipping this would kill a transfer mid-write instead.
/// A closed receiver (supervisor already gone) is not an error.
pub fn stop_supervisor() {
    if let Some(stop_tx) = supervisor_slot().take() {
        let _ = stop_tx.send(true);
    }
}

fn supervisor_slot() -> MutexGuard<'static, Option<tokio::sync::watch::Sender<bool>>> {
    SUPERVISOR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether folder sync can run here — the `CapabilitiesVm.sync` answer.
///
/// Deliberately cheap and side-effect-free: it asks whether a `git` binary
/// exists, not whether the engine has been built, so the capability handshake
/// never pays for opening a database.
pub fn is_available() -> bool {
    cfg!(desktop) && find_git().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_core::error::CoreError;

    #[test]
    fn a_host_label_is_always_produced_and_is_a_short_name() {
        // Provenance identifies the machine; an empty or dotted label makes
        // every commit trailer either useless or noisy.
        let label = read_host_label();
        assert!(!label.is_empty());
        assert!(
            !label.contains('.'),
            "expected a short label, got {label:?}"
        );
    }

    #[test]
    fn git_is_found_on_a_machine_that_has_it() {
        // The whole sync surface is gated on this answer, so a false negative
        // silently removes the feature.
        let found = find_git();
        let on_path = std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert_eq!(
            found.is_some(),
            on_path,
            "find_git disagreed with whether git actually runs here"
        );
    }

    #[test]
    fn availability_tracks_the_git_probe() {
        assert_eq!(is_available(), cfg!(desktop) && find_git().is_some());
    }

    // --- Story 29.6: the desktop end of "notify exactly once, on onset" -----
    //
    // The engine's half (at most one raise per `None -> Some` onset) is covered
    // by `a_warning_notifies_once_per_onset_not_once_per_tick` in `keeper-sync`.
    // What follows covers the leg the engine cannot see: that the onset the
    // engine raised actually reaches the OS notifier, once, unaltered, and with
    // the loud-failure posture AD-39 requires.

    /// A capturing [`Platform`] double recording every `(title, body, target)`
    /// posted through `notify` (mirrors `keeper-core::notify`'s test double).
    ///
    /// Unlike that one, the failing variant records the attempt *before*
    /// erroring: the swallow contract is "attempted once and logged", and a
    /// double that recorded nothing could not tell that apart from a shell that
    /// never called the notifier at all.
    struct CapturingPlatform {
        calls: Mutex<Vec<(String, String, NotifyTarget)>>,
        fail: bool,
    }

    impl CapturingPlatform {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                fail: false,
            }
        }
        fn failing() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                fail: true,
            }
        }
        /// The `(title, body)` of every posted notification (the target is
        /// asserted separately via [`CapturingPlatform::targets`]).
        fn calls(&self) -> Vec<(String, String)> {
            self.calls
                .lock()
                .expect("lock calls")
                .iter()
                .map(|(title, body, _)| (title.clone(), body.clone()))
                .collect()
        }
        /// The click-through [`NotifyTarget`] of every posted notification.
        fn targets(&self) -> Vec<NotifyTarget> {
            self.calls
                .lock()
                .expect("lock calls")
                .iter()
                .map(|(_, _, target)| target.clone())
                .collect()
        }
    }

    impl Platform for CapturingPlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(PathBuf::from("/tmp/keeper-sync-test"))
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
        fn notify(&self, title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError> {
            self.calls.lock().expect("lock calls").push((
                title.to_owned(),
                body.to_owned(),
                target.clone(),
            ));
            if self.fail {
                return Err(CoreError::Unsupported("notify failed in test".to_owned()));
            }
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

    /// The exact shape `Engine::warn` posts on a warning onset.
    const ONSET_TITLE: &str = "Sync — Notes";
    const ONSET_BODY: &str = "Push rejected — the remote moved ahead.";

    #[test]
    fn a_warning_onset_reaches_the_notifier_exactly_once_and_unaltered() {
        let notifier = Arc::new(CapturingPlatform::new());
        let shell = ShellSyncPlatform::new(notifier.clone());

        shell.notify(ONSET_TITLE, ONSET_BODY);

        // The engine already owns "how often"; the shell must neither drop the
        // onset nor turn one raise into two, and the copy the user reads is the
        // engine's, not a re-worded one.
        assert_eq!(
            notifier.calls(),
            vec![(ONSET_TITLE.to_owned(), ONSET_BODY.to_owned())]
        );
    }

    #[test]
    fn a_warning_onset_is_posted_untargeted_as_a_loud_failure() {
        let notifier = Arc::new(CapturingPlatform::new());
        let shell = ShellSyncPlatform::new(notifier.clone());

        shell.notify(ONSET_TITLE, ONSET_BODY);

        // AD-39: a stalled sync is a loud failure, posted on the same
        // untargeted path as recording faults rather than routed through a
        // click-through target that would make it a quiet, dismissible nudge.
        assert_eq!(notifier.targets(), vec![NotifyTarget::None]);
    }

    #[test]
    fn a_notifier_that_fails_does_not_fail_the_sync() {
        let notifier = Arc::new(CapturingPlatform::failing());
        let shell = ShellSyncPlatform::new(notifier.clone());

        shell.notify(ONSET_TITLE, ONSET_BODY);
        shell.notify(ONSET_TITLE, ONSET_BODY);

        // Best-effort by contract: a machine with no working notifier still
        // syncs — neither call may panic or unwind. Each raise is attempted
        // exactly once (a failure is logged, never retried into a duplicate)
        // and a first failure must not latch the path shut for the next one.
        assert_eq!(notifier.calls().len(), 2);
    }
}
