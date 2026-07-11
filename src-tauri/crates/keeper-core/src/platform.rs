//! Platform port trait (AD-24).
//!
//! `keeper-core` reaches the OS only through this port. The concrete
//! implementation lives in the `keeper` shell crate, keeping the hexagon free
//! of any platform- or tauri-specific dependency. Story 1.1 wires the data-dir
//! port end-to-end; the remaining ports (keychain, notifier, sidecar) are
//! declared here and return [`CoreError::Unsupported`] from the shell impl for
//! now — honest and non-panicking — until later stories fill them.

use std::path::{Path, PathBuf};

use crate::error::CoreError;
use crate::vm::NotifyTarget;

/// Injected platform capabilities the core depends on.
///
/// Implementations must be `Send + Sync` so they can be shared across the
/// account-supervision tasks introduced in later stories.
pub trait Platform: Send + Sync {
    /// Root directory for keeper's own persistent data (databases, account
    /// stores). Fully wired in Story 1.1.
    fn data_dir(&self) -> Result<PathBuf, CoreError>;

    /// Store a secret (e.g. an access token) in the OS keychain under `key`.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain,
    /// service `dev.tgorka.keeper`, backed by `keyring`).
    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError>;

    /// Retrieve a secret previously stored under `key`, if present.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain).
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError>;

    /// Delete a secret stored under `key`.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain).
    fn keychain_delete(&self, key: &str) -> Result<(), CoreError>;

    /// Open `url` in the user's default system browser (Story 2.2, OIDC login).
    ///
    /// Used by the OIDC `AuthProvider` to present the OAuth authorization URL for
    /// browser consent. Kept as a port so `keeper-core` stays tauri-free — the
    /// concrete browser-open lives in the `keeper` shell.
    fn open_url(&self, url: &str) -> Result<(), CoreError>;

    /// Post a desktop notification (title + body) carrying a typed click-through
    /// [`NotifyTarget`] (Story 10.4, FR-51). The kept desktop backend has no
    /// per-notification click callback, so the shell records `target` as the "last
    /// notification target" at dispatch and drives a coarse view landing on app
    /// activation (Message → Inbox, Bridge → Bridges); it must NEVER be presented as
    /// exact-message routing. Not wired in Story 1.1 — returns [`CoreError::Unsupported`].
    fn notify(&self, title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError>;

    /// Resolve the path to a bundled sidecar binary by logical `name`.
    /// Not wired in Story 1.1 — returns [`CoreError::Unsupported`].
    fn sidecar_path(&self, name: &str) -> Result<PathBuf, CoreError>;

    /// Exclude `path` (a file or directory under [`Platform::data_dir`]) from OS
    /// device backups (Story 14.7, FR-65).
    ///
    /// On iOS this sets `NSURLIsExcludedFromBackupKey` on the file/directory URL
    /// so the path never reaches iCloud/iTunes device backups; **directory-level
    /// exclusion covers the whole subtree**, which is how the SQLite `-wal`/`-shm`
    /// sidecars next to each `.db` are kept out of backup (callers flag the
    /// containing directory, never a bare `.db` file). On desktop there is no
    /// equivalent per-path backup-exclusion concept, so the port is an honest
    /// no-op returning `Ok(())`. Callers pass absolute, already-created paths
    /// rooted under `data_dir`, and must treat a failure as best-effort hardening
    /// to log and swallow — never a reason to abort startup, login, or restore.
    fn exclude_from_backup(&self, path: &Path) -> Result<(), CoreError>;

    /// Set (or clear) the OS dock badge (Story 10.3, FR-53). `Some(n)` shows the
    /// count `n`; `None` clears the badge. Driven from the Rust-computed
    /// cross-account unread/mention aggregate so it stays correct while the window
    /// is hidden — the count is never derived in the webview. The desktop shell
    /// wires this to the main window's badge; when no app handle is available
    /// (headless / tests) it is an honest no-op, never a panic.
    fn set_badge_count(&self, count: Option<u32>) -> Result<(), CoreError>;
}

/// Best-effort [`Platform::exclude_from_backup`]: log-and-continue on failure
/// (Story 14.7, FR-65).
///
/// Backup exclusion is privacy hardening, not a correctness precondition — a
/// failure must never panic the app, abort login or session-restore, or abort
/// the archive path (which is invariant-bound to never abort startup). Every
/// store-creation site calls through this funnel so no exclusion error can be
/// `?`-propagated into a fatal path.
pub(crate) fn exclude_from_backup_best_effort(platform: &dyn Platform, path: &Path) {
    if let Err(error) = platform.exclude_from_backup(path) {
        tracing::warn!(
            path = %path.display(),
            %error,
            "could not exclude path from device backup; continuing (best-effort hardening)"
        );
    }
}
