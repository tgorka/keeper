//! Desktop wiring for the folder-sync engine (Epic 29, AD-40/AD-51).
//!
//! `keeper-sync` deliberately knows nothing about Tauri, so everything
//! platform-shaped lives here: resolving `git`, probing free space, reading the
//! clock, bridging secrets to the Keychain, and owning the supervisor task. The
//! engine holds policy; this module holds the OS.
//!
//! # Availability is a runtime fact, not a build flag
//!
//! The engine cannot exist without a usable `git` (AD-41), so it is built lazily
//! and its absence is reported honestly through `CapabilitiesVm.sync` rather
//! than by shipping surfaces that fail when pressed. A machine with no usable
//! git has no sync UI — the AD-27 "no dead buttons" rule.
//!
//! **Usable**, not present. [`git_resolution`] probes candidates in `PATH` order
//! and answers with the first that clears the engine's version floor, so the
//! capability and `Engine::open` cannot disagree; [`git_report`] is what tells a
//! person which binary won, or why none did.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_core::platform::Platform;
use keeper_core::vm::NotifyTarget;
use keeper_sync::engine::Engine;
use keeper_sync::git::resolve::{GitRequest, GitResolution};
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
        git_resolution(self.platform.as_ref()).program()
    }

    fn host_label(&self) -> String {
        self.host_label.clone()
    }
}

/// Extra places to look for `git` after `PATH` is exhausted.
///
/// A macOS app launched from Finder inherits `launchctl`'s `PATH`
/// (`/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`), not the user's shell's, so
/// a bare `PATH` search reports "no git" on a machine that plainly has one in
/// Homebrew. These come *after* `PATH` so a user who put a git first still gets
/// it — and, because resolution probes, a Homebrew 2.52 is now reached even when
/// `launchctl`'s `PATH` puts a broken `/usr/local/bin/git` ahead of it.
const EXTRA_GIT_LOCATIONS: [&str; 3] = [
    "/opt/homebrew/bin/git",
    "/usr/local/bin/git",
    "/usr/bin/git",
];

/// What to do about a machine with no usable `git`, in the app's own terms.
///
/// Named rather than inlined for the reason `keeper-syncd`'s install advice is:
/// it is the most likely message a person will ever see from this subsystem, and
/// a refusal without a next step is a support ticket. Both halves matter — the
/// version floor, and the fact that a shadowed `PATH` is fixable from Settings
/// without touching the machine's git at all.
pub const GIT_ADVICE: &str = "install git 2.42 or newer (`brew install git`, or \
     `xcode-select --install`), or point keeper at one in Settings → Sync";

/// Candidate `git` paths, in the order they will be probed.
fn git_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("git"))
                .collect()
        })
        .unwrap_or_default();
    candidates.extend(EXTRA_GIT_LOCATIONS.iter().map(PathBuf::from));
    // A `PATH` that already names one of the well-known locations would
    // otherwise be probed twice, spawning a process to learn what we just
    // learned.
    candidates.dedup();
    candidates
}

/// The resolution this process is using, computed at most once per outcome.
///
/// **Why cached.** Resolution spawns one process per candidate up to the winner,
/// and it gates a UI surface: `capabilities()` asks on every window handshake and
/// the Settings report asks on every open. Re-searching a 12-entry `PATH` each
/// time would spawn twelve `git --version` calls to answer a question whose
/// answer had not changed.
///
/// **When it is thrown away.** Two triggers, and no timer:
///
/// * The explicit-path setting changes — [`invalidate_git_resolution`] runs from
///   the setter, so a person who fixes a shadowed `PATH` sees the effect on the
///   next read rather than after a relaunch.
/// * The cached answer is a *refusal*. A refusal is not cached at all, because
///   the fix for it happens outside this process — `brew install git` — and a
///   cache that kept saying "no git" after the user installed one would be the
///   same class of bug as the one this story fixes, just quieter. A success is
///   sticky: the chosen path cannot become unusable by someone upgrading git,
///   and if the binary is deleted the next real `git` call fails loudly with
///   git's own diagnostic rather than a stale capability.
///
/// So the cost of a broken machine is one search per capability read, which is
/// exactly the machine where a person is waiting for an answer.
static GIT: LazyLock<Mutex<Option<CachedGit>>> = LazyLock::new(|| Mutex::new(None));

/// A cached successful resolution, and the setting it was computed for.
struct CachedGit {
    /// The explicit path in force when this was resolved; `None` = automatic.
    /// Compared on read so a setting change cannot be served a stale answer even
    /// if some future caller forgets to invalidate.
    requested: Option<String>,
    resolution: GitResolution,
}

fn git_slot() -> MutexGuard<'static, Option<CachedGit>> {
    GIT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Forget the cached resolution. Called when the explicit path is written.
pub fn invalidate_git_resolution() {
    git_slot().take();
}

/// Resolve the `git` this installation will drive.
///
/// An explicitly configured path is used **exactly**: when it does not clear the
/// floor this refuses and reports why, and never quietly searches `PATH` for a
/// substitute. Silent substitution is the defect this story exists to remove,
/// and replacing it with a different silent substitution would not be a fix.
pub fn git_resolution(platform: &dyn Platform) -> GitResolution {
    let requested = configured_git_path(platform);
    {
        let guard = git_slot();
        if let Some(cached) = guard.as_ref() {
            if cached.requested == requested {
                return cached.resolution.clone();
            }
        }
    }

    let resolution = match &requested {
        Some(path) => GitRequest::explicit(PathBuf::from(path), GIT_ADVICE).resolve(),
        None => GitRequest::search(git_candidates(), GIT_ADVICE).resolve(),
    };
    if resolution.chosen().is_some() {
        *git_slot() = Some(CachedGit {
            requested,
            resolution: resolution.clone(),
        });
    }
    resolution
}

/// The explicitly chosen `git`, or `None` for automatic resolution.
///
/// A read failure is `None` — automatic — and is logged rather than swallowed: a
/// `keeper.db` that cannot be read is a real fault, but making it *also* mean "no
/// sync at all" would turn one problem into two.
pub fn configured_git_path(platform: &dyn Platform) -> Option<String> {
    let data_dir = platform.data_dir().ok()?;
    match keeper_core::registry::get_sync_git_path(&data_dir) {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(%err, "sync: could not read the configured git path; searching PATH");
            None
        }
    }
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
/// Best-effort, and **loud** when it cannot start. A failed engine build here is
/// almost always a `git` that keeper cannot use, and until Story 34.14 that was
/// a `debug!` line — a level nobody turns on — so from the outside sync simply
/// did nothing, with no record of why. The refusal now carries the resolver's
/// full sentence (which candidates were tried, what each said) at `warn`.
/// Idempotent; safe to call before any profile exists (the tick is a no-op then).
pub fn start_supervisor(platform: Arc<dyn Platform>) {
    let mut guard = supervisor_slot();
    if guard.is_some() {
        return;
    }
    let engine = match engine(platform) {
        Ok(engine) => engine,
        Err(err) => {
            tracing::warn!(%err, "sync: no background sync on this machine");
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
    fn the_candidate_list_is_path_order_then_the_well_known_locations() {
        // Order is the whole fix: probing in `PATH` order is what lets a
        // Homebrew 2.52 win over a `/usr/local/bin/git` that `launchctl`'s PATH
        // puts first, while a user who deliberately put a git first still gets it.
        let candidates = git_candidates();
        let from_path: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("git"))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            candidates.starts_with(&from_path),
            "PATH must be probed before the fallbacks"
        );
        for extra in EXTRA_GIT_LOCATIONS {
            assert!(
                candidates.iter().any(|c| c == Path::new(extra)),
                "a Finder-launched app needs {extra} as a fallback"
            );
        }
    }

    /// A `Platform` whose data dir is a fresh temp tree, so the `sync.git_path`
    /// setting can be written and read for real rather than mocked.
    struct SettingsPlatform {
        data_dir: PathBuf,
    }

    impl Platform for SettingsPlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(self.data_dir.clone())
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
        fn notify(&self, _t: &str, _b: &str, _target: &NotifyTarget) -> Result<(), CoreError> {
            Ok(())
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("unused".to_owned()))
        }
        fn exclude_from_backup(&self, _path: &Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn set_badge_count(&self, _count: Option<u32>) -> Result<(), CoreError> {
            Ok(())
        }
    }

    /// A private data dir for one test. `tempfile` is not a dependency of this
    /// crate; `std::env::temp_dir()` plus the pid is the convention here.
    fn test_data_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keeper-git-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test data dir");
        dir
    }

    /// A fake `git` that answers `--version` with `version`.
    fn fake_git(dir: &Path, name: &str, version: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let program = dir.join(name);
        std::fs::write(
            &program,
            format!("#!/bin/sh\necho 'git version {version}'\n"),
        )
        .expect("fixture");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("mode");
        program
    }

    /// One test, not three, because [`GIT`] is process-global: two tests that
    /// each invalidated it would race each other's cache assertions.
    #[test]
    fn a_configured_git_is_obeyed_exactly_and_its_success_is_cached() {
        let dir = test_data_dir("configured");
        let platform = SettingsPlatform {
            data_dir: dir.clone(),
        };

        // --- Below the floor: refuse, and do NOT fall back --------------------
        // Replacing one silent substitution with another is not a fix, so a
        // named binary that cannot serve must refuse even though this very
        // machine has a usable git on `PATH` to fall back onto.
        let old = fake_git(&dir, "git-2.23", "2.23.0");
        keeper_core::registry::set_sync_git_path(&dir, &old.display().to_string())
            .expect("set old");
        invalidate_git_resolution();

        let resolution = git_resolution(&platform);
        assert!(resolution.is_explicit());
        assert!(resolution.chosen().is_none(), "no fallback to PATH");
        assert_eq!(
            resolution.program().expect_err("must refuse").code(),
            "gitMissing"
        );
        let refusal = resolution.refusal();
        assert!(refusal.contains("2.23"), "{refusal}");
        assert!(refusal.contains(GIT_ADVICE), "{refusal}");

        // --- Clearing the setting searches again, in this same process --------
        keeper_core::registry::set_sync_git_path(&dir, "").expect("clear");
        invalidate_git_resolution();
        assert!(!git_resolution(&platform).is_explicit());

        // --- Above the floor: used, and the success is sticky -----------------
        let good = fake_git(&dir, "git-2.52", "2.52.0");
        keeper_core::registry::set_sync_git_path(&dir, &good.display().to_string())
            .expect("set good");
        invalidate_git_resolution();
        assert_eq!(git_resolution(&platform).program().expect("chosen"), good);

        // Deleting the binary must not change the answer within this process: a
        // success is cached, and the next real `git` call is what reports a
        // binary that went away — with git's own diagnostic, not a stale flag.
        std::fs::remove_file(&good).expect("remove");
        assert_eq!(git_resolution(&platform).program().expect("cached"), good);

        // A refusal, by contrast, is never cached, which is what lets
        // `brew install git` take effect without relaunching the app.
        invalidate_git_resolution();
        assert!(git_resolution(&platform).chosen().is_none());

        invalidate_git_resolution();
        let _ = std::fs::remove_dir_all(&dir);
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
