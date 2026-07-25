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
}
